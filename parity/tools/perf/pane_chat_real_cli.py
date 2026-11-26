#!/usr/bin/env python3
"""실제 Claude Code로 pane 채팅 자동 연결을 확인한다. Claude 버전을 올릴 때 다시 돌리는 시나리오다.

앱은 `pane_chat_latency.Session`과 같은 격리 조건(fixture HOME·XDG_*·TMUX_TMPDIR·hook 포트, Xvfb)으로 띄운다. 실제 CLI만
실제 HOME으로, 격리 XDG_*를 지우고 `DISABLE_AUTOUPDATER=1`로 띄운다(격리 XDG_DATA_HOME을 물려받으면 Claude가
`~/.local/bin/claude` 링크를 fixture 경로로 바꾼다). 앱은 fixture HOME의 `.claude/sessions`·`.claude/projects`를 실제 경로에
연결한 링크로 본다. 모델은 작은 모델(`--model haiku`)만 쓴다.

시나리오(각 1회, fixture·앱 프로세스를 새로 만든다)
- trust-order: tmux task에서 처음 쓰는 폴더의 신뢰 화면을 10초 둔다. 그동안 세션 기록과 `pane_chat:open`이 없고,
  수락해 기록이 생긴 뒤 `pane_chat:ready`가 나오는지
- zellij-claude: Zellij task에서 새 대화가 자동으로 열리고 첫 프롬프트의 답이 채팅에 보이는지
- herdr-claude: Herdr task에서 새 대화가 자동으로 열리고, 첫 프롬프트·`/clear`·`/resume`이 채팅을 따라가는지

끝나면 이 실행이 만든 `~/.claude/projects/<작업 폴더>` 디렉터리와 끝난 CLI의 세션 기록만 지우고, `~/.local/bin/claude` 링크와
`claude --version`이 시작 전과 같은지 본다. `~/.claude.json`·`history.jsonl`은 여러 세션이 쓰는 파일이라 고치지 않는다.

예:
  python3 parity/tools/perf/pane_chat_real_cli.py --app /var/tmp/kanvibe --scenario trust-order
결과: qa-results/roky-harness/pane-chat-latency/real-cli/<커밋>/<시나리오>.json, .png, -logs/
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import signal
import sqlite3
import subprocess
import sys
import threading
import time
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS_DIR))

import pane_chat_latency as harness  # noqa: E402

REAL_HOME = Path.home()
REAL_SESSIONS = REAL_HOME / ".claude" / "sessions"
REAL_PROJECTS = REAL_HOME / ".claude" / "projects"
CLAUDE_LINK = REAL_HOME / ".local" / "bin" / "claude"
HERDR_TASK_ID = "22222222-2222-4222-8222-000000000020"
TRUST_OBSERVE_S = 10.0
FIRST_PROMPT = "Reply with exactly the word ALPHA and nothing else."
SCENARIO_TASKS = {"trust-order": "tmux", "zellij-claude": "zellij", "herdr-claude": "herdr"}


def claude_command() -> str:
    """pane 셸에 입력할 실제 Claude 실행 줄. 격리 XDG_*를 지워 실제 설정·데이터 경로를 쓰게 한다"""
    unset = " ".join(f"-u {name}" for name in
                     ("XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME", "XDG_RUNTIME_DIR"))
    return f"env {unset} DISABLE_AUTOUPDATER=1 HOME={REAL_HOME} {CLAUDE_LINK} --model haiku"


def link_real_claude_records(home: Path) -> None:
    """앱이 실제 Claude의 세션 기록과 대화 기록을 보도록 fixture HOME의 두 디렉터리를 실제 경로에 잇는다"""
    claude = home / ".claude"
    claude.mkdir(parents=True, exist_ok=True)
    for name in ("sessions", "projects"):
        target = claude / name
        if target.is_symlink() or target.is_file():
            target.unlink()
        elif target.exists():
            shutil.rmtree(target)
        target.symlink_to(REAL_HOME / ".claude" / name)


class RecordWatch:
    """실제 세션 기록을 2ms마다 읽어 CLI(pid)의 대화 ID가 바뀐 시각과 그 기록의 cwd를 남긴다.
    `/resume`은 전에 쓴 대화 ID로 돌아가므로 (pid, 대화 ID)를 처음 본 시각이 아니라 바뀔 때마다 남긴다"""

    def __init__(self) -> None:
        self.changes: list[tuple[str, str, int]] = []
        self.current: dict[str, str] = {}
        self.cwd_of: dict[str, str] = {}
        self.lock = threading.Lock()
        threading.Thread(target=self.follow, daemon=True).start()

    def follow(self) -> None:
        while True:
            now = time.time_ns()
            for path in REAL_SESSIONS.glob("*.json"):
                try:
                    record = json.loads(path.read_text())
                except (OSError, ValueError):
                    continue
                with self.lock:
                    self.cwd_of[path.stem] = record.get("cwd")
                    if self.current.get(path.stem) != record.get("sessionId"):
                        self.current[path.stem] = record.get("sessionId")
                        self.changes.append((path.stem, record.get("sessionId"), now))
            time.sleep(0.002)

    def known_pids(self) -> set[str]:
        with self.lock:
            return set(self.current)

    def records_for(self, cwd: str) -> list[tuple[str, str, int]]:
        with self.lock:
            return [change for change in self.changes if self.cwd_of.get(change[0]) == cwd]

    def wait(self, cwd: str, predicate, timeout_s: float = 60.0, after_ns: int = 0) -> tuple[str, str, int]:
        deadline = time.monotonic() + timeout_s
        while time.monotonic() < deadline:
            for pid, session_id, seen in self.records_for(cwd):
                if seen >= after_ns and predicate(pid, session_id):
                    return pid, session_id, seen
            time.sleep(0.005)
        raise TimeoutError("claude session record not seen")


# ───────────── pane 입력과 화면 ─────────────

class TmuxPane:
    def __init__(self, session: harness.Session, target: str) -> None:
        self.environment = {name: session.environment[name] for name in ("PATH", "HOME", "TMUX_TMPDIR")}
        self.target = target

    def run(self, *arguments: str) -> str:
        return subprocess.run(["tmux", *arguments], env=self.environment, capture_output=True, text=True).stdout

    def send_text(self, text: str) -> None:
        self.run("send-keys", "-t", self.target, "-l", text)

    def send_key(self, key: str) -> None:
        self.run("send-keys", "-t", self.target, {"enter": "Enter", "down": "Down"}[key])

    def screen(self) -> str:
        return self.run("capture-pane", "-p", "-t", self.target)


class ZellijPane:
    def __init__(self, session: harness.Session, session_name: str) -> None:
        self.environment = session.environment
        self.session_name = session_name
        self.dump = session.work / "zellij-screen.txt"

    def run(self, *arguments: str) -> str:
        return subprocess.run(["zellij", "--session", self.session_name, "action", *arguments],
                              env=self.environment, capture_output=True, text=True).stdout

    def send_text(self, text: str) -> None:
        self.run("write-chars", text)

    def send_key(self, key: str) -> None:
        self.run("write", *{"enter": ["13"], "down": ["27", "91", "66"]}[key])

    def screen(self) -> str:
        self.run("dump-screen", "--path", str(self.dump))
        return self.dump.read_text() if self.dump.exists() else ""


class HerdrPane:
    """fixture task의 Herdr 서버에서 focus된 pane. 환경을 명시해 사용자 Herdr 서버는 보이지 않는다"""

    def __init__(self, session: harness.Session) -> None:
        self.environment = {name: session.environment[name] for name in ("PATH", "HOME", "XDG_CONFIG_HOME")}
        with sqlite3.connect(session.fixture["database"]) as database:
            (session_name,) = database.execute("SELECT session_name FROM kanban_tasks WHERE id = ?",
                                               (HERDR_TASK_ID,)).fetchone()
        self.server = "kanvibe" if session_name.startswith("kanvibe:") else session_name
        panes = json.loads(self.run("pane", "list"))["result"]["panes"]
        self.pane_id = next(pane["pane_id"] for pane in panes if pane.get("focused"))

    def run(self, *arguments: str) -> str:
        return subprocess.run(["herdr", "--session", self.server, *arguments], env=self.environment,
                              capture_output=True, text=True).stdout

    def send_text(self, text: str) -> None:
        self.run("pane", "send-text", self.pane_id, text)

    def send_key(self, key: str) -> None:
        self.run("pane", "send-keys", self.pane_id, key)

    def screen(self) -> str:
        return self.run("pane", "read", self.pane_id, "--source", "visible")


def send_line(pane, text: str) -> None:
    pane.send_text(text)
    time.sleep(0.3)
    pane.send_key("enter")


def wait_screen(pane, text: str, timeout_s: float = 30.0) -> None:
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        if text in pane.screen():
            return
        time.sleep(0.25)
    raise TimeoutError(f"pane screen did not show {text!r}")


def accept_trust(pane) -> None:
    """신뢰 화면의 기본 선택은 "No, exit"이므로 "Yes, I trust this folder"로 옮긴 뒤에만 Enter를 보낸다"""
    for _ in range(10):
        if "❯ Yes" in pane.screen():
            pane.send_key("enter")
            return
        pane.send_key("down")
        time.sleep(0.5)
    raise RuntimeError("trust prompt selection did not move to Yes")


def markers_since(session: harness.Session, after_ns: int) -> list[str]:
    with session.markers.condition:
        return [name for name, at in session.markers.markers if at >= after_ns]


def ms_after(session: harness.Session, name: str, after_ns: int, timeout_s: float = 30.0) -> float:
    return (session.marker(name, after_ns, timeout_s) - after_ns) / 1e6


def wait_chat_text(session: harness.Session, text: str, timeout_ms: int) -> None:
    """채팅 메시지에 그 글이 들어 있을 때까지 기다린다. 답 줄은 정확히 같은 글의 대상으로 잡히지 않아 프롬프트 글로 본다"""
    session.send({"cmd": "wait_for", "target": {"text": text, "exact": False}, "timeout_ms": timeout_ms})


# ───────────── 시나리오 ─────────────

def scenario_trust_order(session: harness.Session, pane, watch: RecordWatch, results: dict) -> None:
    cwd = harness.task_worktree(session)
    known = watch.known_pids()
    send_line(pane, claude_command())
    wait_screen(pane, "I trust this folder")
    shown = time.time_ns()
    time.sleep(TRUST_OBSERVE_S)
    during = markers_since(session, shown)
    results["trust"] = {"observed_s": TRUST_OBSERVE_S,
                        "record_during_trust": any(pid not in known for pid, _, _ in watch.records_for(cwd)),
                        "open_during_trust": "pane_chat:open" in during,
                        "layouts_during_trust": during.count("pane_chat:layout_done"),
                        "probes_during_trust": during.count("pane_chat:probe_done")}
    accepted = time.time_ns()
    accept_trust(pane)
    pid, session_id, written = watch.wait(cwd, lambda pid, _: pid not in known)
    results["trust"].update({"pid": pid, "session": session_id,
                             "accept_to_record_ms": (written - accepted) / 1e6,
                             "record_to_ready_ms": ms_after(session, "pane_chat:ready", written)})


def start_and_connect(session: harness.Session, pane, watch: RecordWatch, results: dict) -> tuple[str, str]:
    """실제 Claude를 띄워 신뢰를 수락하고, 세션 기록이 생긴 뒤 채팅이 자동으로 열리기까지를 잰다"""
    cwd = harness.task_worktree(session)
    known = watch.known_pids()
    send_line(pane, claude_command())
    wait_screen(pane, "I trust this folder")
    accept_trust(pane)
    pid, session_id, written = watch.wait(cwd, lambda pid, _: pid not in known)
    results["start"] = {"pid": pid, "session": session_id,
                        "record_to_open_ms": ms_after(session, "pane_chat:open", written),
                        "record_to_ready_ms": ms_after(session, "pane_chat:ready", written)}
    time.sleep(3.0)
    send_line(pane, FIRST_PROMPT)
    wait_chat_text(session, FIRST_PROMPT, 90000)
    results["first_prompt_shown_in_chat"] = True
    return pid, session_id


def scenario_zellij_claude(session: harness.Session, pane, watch: RecordWatch, results: dict) -> None:
    start_and_connect(session, pane, watch, results)


def scenario_herdr_claude(session: harness.Session, pane, watch: RecordWatch, results: dict) -> None:
    cwd = harness.task_worktree(session)
    pid, first_id = start_and_connect(session, pane, watch, results)
    time.sleep(2.0)
    sent = time.time_ns()
    send_line(pane, "/clear")
    _, cleared_id, written = watch.wait(cwd, lambda p, sid: p == pid and sid != first_id, after_ns=sent)
    results["clear"] = {"session": cleared_id, "record_to_retarget_ms": ms_after(session, "pane_chat:retarget", written)}
    time.sleep(3.0)
    sent = time.time_ns()
    send_line(pane, "/resume")
    time.sleep(2.5)
    pane.send_key("enter")
    _, resumed_id, written = watch.wait(cwd, lambda p, sid: p == pid and sid == first_id, timeout_s=30, after_ns=sent)
    results["resume"] = {"session": resumed_id, "record_to_retarget_ms": ms_after(session, "pane_chat:retarget", written)}
    wait_chat_text(session, FIRST_PROMPT, 10000)
    results["resume"]["resumed_rows_shown"] = True


SCENARIOS = {"trust-order": scenario_trust_order, "zellij-claude": scenario_zellij_claude,
             "herdr-claude": scenario_herdr_claude}


def open_pane(scenario: str, session: harness.Session):
    if scenario == "zellij-claude":
        harness.dismiss_zellij_welcome(session)
        return ZellijPane(session, "api-server-feat-rate-limit")
    session.send({"cmd": "wait_for", "target": {"text": session.target.shell_prompt, "exact": False},
                  "timeout_ms": 40000})
    if scenario == "herdr-claude":
        return HerdrPane(session)
    return TmuxPane(session, "kanvibe-demo-feat-board-filters")


# ───────────── 실제 환경 확인과 뒷정리 ─────────────

def claude_installation() -> dict:
    version = subprocess.run([str(CLAUDE_LINK), "--version"], capture_output=True, text=True,
                             env={"PATH": "/usr/bin:/bin", "HOME": str(REAL_HOME), "DISABLE_AUTOUPDATER": "1"})
    return {"link": os.readlink(CLAUDE_LINK), "version": version.stdout.strip()}


def stop_processes_under(work: Path) -> list[int]:
    """작업 폴더 안에서 돌던 프로세스(멀티플렉서가 끝나도 남은 실제 CLI)를 끈다"""
    stopped = []
    for proc in Path("/proc").glob("[0-9]*"):
        try:
            cwd = os.readlink(proc / "cwd")
        except OSError:
            continue
        if cwd.startswith(str(work) + "/") and int(proc.name) != os.getpid():
            try:
                os.kill(int(proc.name), signal.SIGTERM)
                stopped.append(int(proc.name))
            except ProcessLookupError:
                pass
    return stopped


def remove_run_records(work: Path, watch: RecordWatch, projects_before: set[str]) -> dict:
    """이 실행의 작업 폴더에서 만든 대화 디렉터리와 끝난 CLI의 세션 기록만 지운다"""
    encoded_work = str(work).replace("/", "-").replace("_", "-")
    removed_projects = []
    for directory in REAL_PROJECTS.iterdir():
        if directory.name not in projects_before and directory.name.startswith(encoded_work):
            shutil.rmtree(directory)
            removed_projects.append(directory.name)
    removed_records = []
    with watch.lock:
        pids = {pid for pid, cwd in watch.cwd_of.items() if cwd and cwd.startswith(str(work) + "/")}
    for pid in pids:
        record = REAL_SESSIONS / f"{pid}.json"
        if record.exists() and not Path(f"/proc/{pid}").exists():
            record.unlink()
            removed_records.append(record.name)
    return {"projects": removed_projects, "session_records": removed_records}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--app", required=True, type=Path)
    parser.add_argument("--scenario", required=True, choices=sorted(SCENARIOS))
    parser.add_argument("--out", type=Path, help="기본 qa-results/roky-harness/pane-chat-latency/real-cli/<커밋>")
    parser.add_argument("--work", type=Path, default=Path("/var/tmp/kvrc"))
    args = parser.parse_args()
    commit = subprocess.run(["git", "rev-parse", "--short", "HEAD"], cwd=harness.REPO_ROOT,
                            capture_output=True, text=True).stdout.strip()
    out = args.out or harness.OUTPUT_ROOT / "real-cli" / commit
    # Claude는 신뢰한 폴더를 ~/.claude.json에 남기므로, 실행마다 처음 쓰는 폴더가 되도록 이름에 시각을 붙인다.
    # fixture 안의 Herdr·Zellij 소켓 경로가 유닉스 소켓 길이 한도(108바이트)를 넘지 않도록 이름을 짧게 둔다
    work = args.work / f"{args.scenario[:2]}{time.strftime('%H%M%S')}"
    installation_before = claude_installation()
    projects_before = {directory.name for directory in REAL_PROJECTS.iterdir()}
    watch = RecordWatch()
    time.sleep(0.5)
    target = harness.TASK_TARGETS[SCENARIO_TASKS[args.scenario]]

    def prepare_home(home: Path) -> None:
        link_real_claude_records(home)
        if target is harness.TASK_TARGETS["zellij"]:
            harness.quiet_zellij_first_run(home)

    results: dict = {"scenario": args.scenario, "commit": commit}
    error = None
    session = harness.Session(args.app.resolve(), work, {"pane_chat_default_enabled": "true"}, target=target,
                              prepare_home=prepare_home)
    try:
        SCENARIOS[args.scenario](session, open_pane(args.scenario, session), watch, results)
    except Exception as failure:  # noqa: BLE001 - 실패해도 화면과 로그를 남기고 뒷정리한다
        error = repr(failure)[-600:]
    finally:
        out.mkdir(parents=True, exist_ok=True)
        try:
            harness.capture.grab_window(int(session.environment["DISPLAY"][1:]), *harness.WINDOW_SIZE,
                                        out / f"{args.scenario}.png")
        except Exception:  # noqa: BLE001
            pass
        session.close()
        results["stopped_leftover_pids"] = stop_processes_under(work)
        time.sleep(1.0)
        if (work / "logs").exists():
            shutil.copytree(work / "logs", out / f"{args.scenario}-logs", dirs_exist_ok=True)
        results["removed"] = remove_run_records(work, watch, projects_before)
        shutil.rmtree(work, ignore_errors=True)
    installation_after = claude_installation()
    results["claude_installation"] = {"before": installation_before, "after": installation_after,
                                      "is_unchanged": installation_before == installation_after}
    if not results["claude_installation"]["is_unchanged"] and error is None:
        error = "claude installation changed"
    results["error"] = error
    (out / f"{args.scenario}.json").write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(results, ensure_ascii=False, indent=2))
    return 0 if error is None else 1


if __name__ == "__main__":
    sys.exit(main())
