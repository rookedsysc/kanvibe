#!/usr/bin/env python3
"""pane 채팅의 열기·연결 지연을 릴리스 앱에서 재고 JSON·Markdown 보고서를 남긴다.

앱은 `run_perf.py`와 같은 격리 조건(스크린샷 fixture, 격리 HOME·XDG_*·TMUX_TMPDIR, Xvfb, `--qa-harness --perf-markers
--kanvibe-hook-port`)으로 띄우고, 단계는 QA 제어 소켓으로 보낸다. 시각은 앱 표식(`KANVIBE_PERF <이름> <epoch ns>`, qa/perf_markers.rs)과
드라이버의 `time.time_ns()`로, 같은 CLOCK_REALTIME이다. 그리기 표식은 그 변화가 그려진 프레임의 다음 프레임 시작에서 나온다.

가짜 CLI는 `pane_chat_fake_claude.py`다. Claude Code 2.1.332처럼 `$HOME/.claude/sessions/<pid>.json`을 남기고, `/clear`·`/resume`을
신호(SIGUSR1·SIGUSR2)로 흉내 낸다. 가짜가 기록을 쓴 시각이 "세션 감지" 시각이다.

구간(세션마다 새 fixture·새 앱 프로세스)
open 세션: tmux pane에서 가짜 CLI가 큰 대화(`--transcript-mb`)를 이어 연 상태
- first_open: 첫 `Mod+Shift+J`(앱 `key_received`) → `pane_chat:frame`·`rows`·`ready`
- toggle_reentry: 채팅을 터미널로 돌렸다가 다시 `Mod+Shift+J` → 같은 표식(화면은 남아 있어 다시 보이기만 한다)
- navigate_reentry: 보드로 갔다가 작업 상세로 돌아옴(저장된 채팅 모드가 CLI를 확인한 뒤 자동으로 연다) → `open`·`rows`·`ready`
- history_open: 채팅의 "히스토리" → `history:frame`·`ready`(세션 목록), 첫 세션 고름 → `history:rows`
connect 세션: 기본 채팅 모드를 켜고 CLI가 없는 pane에서 시작
- auto_open: 가짜 CLI가 세션 기록을 쓴 시각 → `pane_chat:ready`(층이 확인하고 열어 보낼 수 있는 입력창을 그림)
- clear: `/clear`가 새 세션 기록을 쓴 시각 → `pane_chat:retarget`
- resume: `/resume`이 다른 대화를 쓴 시각 → `pane_chat:retarget`
- trust_wait(`--trust-wait`): 가짜 CLI가 신뢰 화면을 띄운 뒤 3.5초 동안 `pane_chat:open`이 없는지, Enter로 수락해 세션 기록을 쓴
  시각 → `pane_chat:ready`. 이때는 auto_open·clear·resume을 재지 않는다

`--task`는 어느 fixture task의 pane에서 재는지 고른다(tmux `board-filters`, zellij `rate-limit`, herdr `dark-mode`).
open 세션은 tmux에서만 돌고, zellij·herdr의 connect 세션은 auto_open(또는 trust_wait)만 잰다.

예:
  python3 parity/tools/perf/pane_chat_latency.py --app /var/tmp/kanvibe --runs 10 --label baseline
결과: qa-results/roky-harness/pane-chat-latency/<label>/result.json, report.md, logs/
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import signal
import statistics
import subprocess
import sys
import threading
import time
import uuid
from pathlib import Path
from typing import NamedTuple

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parents[2]
sys.path.insert(0, str(TOOLS_DIR))
sys.path.insert(0, str(TOOLS_DIR.parent / "screens"))
sys.path.insert(0, str(TOOLS_DIR.parent / "screens-rust"))
sys.path.insert(0, str(TOOLS_DIR.parent / "app-launch"))

import capture  # noqa: E402
from capture_rust import ControlClient  # noqa: E402
from check_release_launch import free_display, free_hook_port  # noqa: E402
from run_perf import MarkerLog, fixed_now_iso  # noqa: E402

OUTPUT_ROOT = REPO_ROOT / "qa-results/roky-harness/pane-chat-latency"
WINDOW_SIZE = (1600, 1000)
TASK_ROUTE = "/ko/task/22222222-2222-4222-8222-000000000010"
SHELL_PROMPT = "feat-board-filters $"
FAKE_READY = "Ready for a prompt"
FAKE_TRUST_PROMPT = "Yes, I trust this folder"
TRUST_WAIT_S = 3.5
MARKER_TIMEOUT_S = 30.0


class TaskTarget(NamedTuple):
    """측정할 fixture task. worktree는 fixture `repos/` 아래 경로다"""
    route: str
    worktree: str
    shell_prompt: str


TASK_TARGETS = {
    "tmux": TaskTarget(TASK_ROUTE, "kanvibe-demo__worktrees/feat-board-filters", SHELL_PROMPT),
    "zellij": TaskTarget("/ko/task/22222222-2222-4222-8222-000000000011", "api-server__worktrees/feat-rate-limit",
                         "feat-rate-limit $"),
    "herdr": TaskTarget("/ko/task/22222222-2222-4222-8222-000000000020", "kanvibe-demo__worktrees/feat-dark-mode",
                        "feat-dark-mode $"),
}


class Session:
    """fixture 하나와 앱 프로세스 하나"""

    def __init__(self, app: Path, work: Path, settings_overrides: dict[str, str],
                 target: TaskTarget = TASK_TARGETS["tmux"], prepare_home=None):
        self.work = work
        self.target = target
        fixture_root = work / "fixture"
        self.fixture = capture.build_fixture(fixture_root, "full", {
            "theme_preference": "dark", "sidebar_default_collapsed": "true", **settings_overrides})
        self.fixture_root = fixture_root
        self.agent = work / "agent"
        self.agent.mkdir(parents=True, exist_ok=True)
        shutil.copy(TOOLS_DIR / "pane_chat_fake_claude.py", self.agent / "claude")
        (self.agent / "claude").chmod(0o755)
        self.events_path = work / "agent-events.jsonl"
        self.events_path.touch()
        # Claude Code를 한 번이라도 실행한 머신처럼 세션 기록 디렉터리를 둔다(처음 실행 전에는 없다)
        (Path(self.fixture["home"]) / ".claude" / "sessions").mkdir(parents=True, exist_ok=True)
        if prepare_home is not None:
            prepare_home(Path(self.fixture["home"]))
        fontconfig_path = capture.write_pinned_fontconfig(work / "fontconfig")
        display = free_display()
        self.environment = capture.build_app_environment(self.fixture, fixture_root, display, fontconfig_path)
        logs = work / "logs"
        logs.mkdir(parents=True, exist_ok=True)
        self.xvfb = capture.start_xvfb(display, *WINDOW_SIZE, logs / "xvfb.log")
        socket_path = work / "control.sock"
        command = [
            str(app), "--qa-harness", "--perf-markers",
            "--data-dir", self.fixture["appdata"],
            "--fixture-root", self.fixture["root"],
            "--fixed-now", fixed_now_iso(),
            "--network-responses", str(capture.FIXTURES_DIR / "network" / "default.json"),
            "--control-socket", str(socket_path),
            "--route", target.route,
            "--window-size", f"{WINDOW_SIZE[0]}x{WINDOW_SIZE[1]}",
            "--kanvibe-hook-port", str(free_hook_port()),
        ]
        self.markers = MarkerLog(logs / "app.stdout.log")
        self.stderr_log = (logs / "app.stderr.log").open("w")
        self.process = subprocess.Popen(command, env=self.environment, cwd=REPO_ROOT, stdout=subprocess.PIPE,
                                        stderr=self.stderr_log, start_new_session=True)
        threading.Thread(target=self.markers.follow, args=(self.process.stdout,), daemon=True).start()
        self.client = ControlClient(socket_path, timeout_s=60)
        self.client.send({"cmd": "wait_idle"})
        self.client.send({"cmd": "activate_window"})

    @property
    def home(self) -> Path:
        return Path(self.fixture["home"])

    def send(self, command: dict) -> dict:
        return self.client.send(command)

    def marker(self, name: str, after_ns: int, timeout_s: float = MARKER_TIMEOUT_S) -> int:
        return self.markers.wait_for({name}, after_ns, timeout_s)[1]

    def key(self, keys: str) -> int:
        """키를 보내고 앱이 받은 시각(`key_received`)을 돌려준다"""
        sent_ns = time.time_ns()
        self.send({"cmd": "key", "keys": keys})
        return self.marker("key_received", sent_ns)

    def start_fake_cli(self, extra: str, ready_text: str = FAKE_READY) -> None:
        self.send({"cmd": "wait_for", "target": {"text": self.target.shell_prompt, "exact": False}, "timeout_ms": 30000})
        self.send({"cmd": "click", "target": {"selector": ".xterm-screen"}})
        self.send({"cmd": "type", "text": f"{self.agent / 'claude'} --events {self.events_path} {extra}"})
        self.send({"cmd": "key", "keys": "Enter"})
        self.send({"cmd": "wait_for", "target": {"text": ready_text, "exact": False}, "timeout_ms": 30000})

    def agent_event(self, event: str, after_ns: int, timeout_s: float = 10.0) -> dict:
        deadline = time.monotonic() + timeout_s
        while time.monotonic() < deadline:
            for line in self.events_path.read_text().splitlines():
                record = json.loads(line)
                if record["event"] == event and record["ns"] >= after_ns:
                    return record
            time.sleep(0.01)
        raise TimeoutError(f"fake cli event {event} not seen")

    def signal_fake_cli(self, signal_number: int) -> None:
        result = subprocess.run(["pgrep", "-f", f"{self.agent / 'claude'} --events"], capture_output=True, text=True)
        for pid_text in result.stdout.split():
            os.kill(int(pid_text), signal_number)

    def close(self) -> None:
        try:
            self.send({"cmd": "quit"})
            self.process.wait(timeout=10)
        except Exception:  # noqa: BLE001 - 끝낼 때는 남은 것을 모두 정리한다
            pass
        if self.process.poll() is None:
            try:
                os.killpg(self.process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        self.process.wait()
        self.stderr_log.close()
        capture.kill_fixture_multiplexers(self.environment)
        capture.kill_leftover_processes(self.fixture_root)
        subprocess.run(["pkill", "-f", str(self.agent)], capture_output=True)
        capture.stop_process(self.xvfb)


# ───────────── 큰 대화 기록 ─────────────

def write_transcript(home: Path, cwd: str, session_id: str, target_bytes: int) -> Path:
    """사용자·어시스턴트가 번갈아 말한 Claude 기록. 길이가 다른 문단으로 target_bytes만큼 채운다"""
    directory = home / ".claude" / "projects" / cwd.replace("/", "-").replace("_", "-")
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / f"{session_id}.jsonl"
    paragraph = "보드 필터를 바꾸는 중입니다. 상태·담당자·라벨을 함께 거르고 결과를 URL에 남깁니다. "
    written = 0
    turn = 0
    with path.open("w", encoding="utf-8") as transcript:
        while written < target_bytes:
            for role in ("user", "assistant"):
                text = f"[{turn}] " + paragraph * (2 if role == "user" else 6 + turn % 20)
                line = json.dumps({"type": role, "sessionId": session_id, "cwd": cwd,
                                   "uuid": str(uuid.uuid4()),
                                   "timestamp": f"2026-01-15T02:{turn // 60 % 60:02d}:{turn % 60:02d}.000Z",
                                   "message": {"role": role, "content": text}}, ensure_ascii=False)
                transcript.write(line + "\n")
                written += len(line.encode("utf-8")) + 1
            turn += 1
    return path


def task_worktree(session: Session) -> str:
    return str(session.fixture_root / "repos" / session.target.worktree)


def quiet_zellij_first_run(home: Path) -> None:
    """격리 XDG_CONFIG_HOME의 Zellij 설정에 시작 안내와 릴리스 노트 창을 끈다(fixture 안에만 쓴다)"""
    config = home.parent / "xdg" / "config" / "zellij" / "config.kdl"
    config.parent.mkdir(parents=True, exist_ok=True)
    config.write_text("show_startup_tips false\nshow_release_notes false\n", encoding="utf-8")


def dismiss_zellij_welcome(session: Session) -> None:
    """Zellij 0.46 첫 실행의 데스크톱 연동 안내는 설정으로 끌 수 없고 Enter가 설치를 승인하므로 Esc(Dismiss)로만 닫는다"""
    session.send({"cmd": "wait_for", "target": {"text": session.target.shell_prompt, "exact": False}, "timeout_ms": 30000})
    session.send({"cmd": "click", "target": {"selector": ".xterm-screen"}})
    try:
        session.send({"cmd": "wait_for", "target": {"text": "Welcome to Zellij", "exact": False}, "timeout_ms": 8000})
    except RuntimeError:
        return
    session.send({"cmd": "key", "keys": "Escape"})
    time.sleep(1.0)


# ───────────── 구간 ─────────────

def paint_segments(session: Session, start_ns: int, screen: str, phases: tuple[str, ...]) -> dict:
    return {phase: (session.marker(f"{screen}:{phase}", start_ns) - start_ns) / 1e6 for phase in phases}


def read_durations(session: Session, after_ns: int, until_ns: int, kind: str) -> list[float]:
    """구간 안의 조회 시간(read_start→read_done 또는 probe_start→probe_done)"""
    start_name, done_name = f"pane_chat:{kind}_start", f"pane_chat:{kind}_done"
    with session.markers.condition:
        markers = [(name, at) for name, at in session.markers.markers if after_ns <= at <= until_ns]
    durations, open_at = [], None
    for name, at in markers:
        if name == start_name:
            open_at = at
        elif name == done_name and open_at is not None:
            durations.append((at - open_at) / 1e6)
            open_at = None
    return durations


def run_open_session(app: Path, work: Path, transcript_mb: float) -> dict:
    session = Session(app, work, {})
    try:
        session_id = str(uuid.uuid4())
        write_transcript(session.home, task_worktree(session), session_id, int(transcript_mb * 1024 * 1024))
        session.start_fake_cli(f"--resume {session_id}")
        time.sleep(1.0)
        result: dict = {}
        start = session.key("Mod+Shift+J")
        result["first_open"] = paint_segments(session, start, "pane_chat", ("layout_done", "frame", "rows", "ready"))
        ready_at = start + int(result["first_open"]["ready"] * 1e6)
        result["first_open"]["reads_ms"] = read_durations(session, start, ready_at, "read")
        result["toggle_reentry"] = []
        for _ in range(2):
            time.sleep(1.0)
            session.key("Mod+Shift+J")
            time.sleep(1.5)
            start = session.key("Mod+Shift+J")
            result["toggle_reentry"].append(
                paint_segments(session, start, "pane_chat", ("layout_done", "frame", "rows", "ready")))
        time.sleep(1.0)
        session.send({"cmd": "navigate", "route": "/ko"})
        time.sleep(1.5)
        start = time.time_ns()
        session.send({"cmd": "navigate", "route": TASK_ROUTE})
        navigate = paint_segments(session, start, "pane_chat", ("open", "frame", "rows", "ready"))
        ready_at = start + int(navigate["ready"] * 1e6)
        navigate["probes_ms"] = read_durations(session, start, ready_at, "probe")
        navigate["reads_ms"] = read_durations(session, start, ready_at, "read")
        result["navigate_reentry"] = navigate
        time.sleep(1.0)
        start = time.time_ns()
        session.send({"cmd": "click", "target": {"role": "button", "name": "히스토리"}})
        result["history_open"] = paint_segments(session, start, "history", ("frame", "ready"))
        time.sleep(0.5)
        start = time.time_ns()
        session.send({"cmd": "click", "target": {"text": f"[0] ", "exact": False}})
        result["history_select"] = paint_segments(session, start, "history", ("frame", "rows"))
        return result
    finally:
        session.close()


def measure_trust_wait(session: Session, resume_id: str) -> dict:
    """신뢰 화면이 떠 있는 동안 채팅이 열리지 않고, 수락해 세션 기록이 생긴 뒤에 열리는지"""
    before = time.time_ns()
    session.start_fake_cli(f"--trust-wait --switch-to {resume_id}", ready_text=FAKE_TRUST_PROMPT)
    shown = session.agent_event("trust_shown", before)["ns"]
    time.sleep(TRUST_WAIT_S)
    accepted = time.time_ns()
    with session.markers.condition:
        during = [name for name, at in session.markers.markers if shown <= at < accepted]
    session.send({"cmd": "key", "keys": "Enter"})
    started = session.agent_event("start", accepted)["ns"]
    return {"open_during_trust": "pane_chat:open" in during,
            "layouts_during_trust": during.count("pane_chat:layout_done"),
            "probes_during_trust": during.count("pane_chat:probe_done"),
            "ready": (session.marker("pane_chat:ready", started) - started) / 1e6}


def run_connect_session(app: Path, work: Path, target: TaskTarget, should_wait_for_trust: bool) -> dict:
    prepare_home = quiet_zellij_first_run if target is TASK_TARGETS["zellij"] else None
    session = Session(app, work, {"pane_chat_default_enabled": "true"}, target=target, prepare_home=prepare_home)
    try:
        resume_id = str(uuid.uuid4())
        write_transcript(session.home, task_worktree(session), resume_id, 64 * 1024)
        if target is TASK_TARGETS["zellij"]:
            dismiss_zellij_welcome(session)
        # 기본 채팅 모드가 CLI 없는 pane을 확인하며 도는 상태로 둔다
        time.sleep(3.0)
        if should_wait_for_trust:
            return {"trust_wait": measure_trust_wait(session, resume_id)}
        before = time.time_ns()
        session.start_fake_cli(f"--switch-to {resume_id}")
        detected = session.agent_event("start", before)["ns"]
        result: dict = {"auto_open": paint_segments(session, detected, "pane_chat", ("open", "ready"))}
        ready_at = detected + int(result["auto_open"]["ready"] * 1e6)
        result["auto_open"]["probes_ms"] = read_durations(session, detected, ready_at, "probe")
        if target is not TASK_TARGETS["tmux"]:
            return result
        result["clear"], result["resume"] = [], []
        for index in range(3):
            time.sleep(2.0)
            before = time.time_ns()
            session.signal_fake_cli(signal.SIGUSR1)
            detected = session.agent_event("clear", before)["ns"]
            result["clear"].append({"retarget": (session.marker("pane_chat:retarget", detected) - detected) / 1e6})
            if index < 2:
                time.sleep(2.0)
                before = time.time_ns()
                session.signal_fake_cli(signal.SIGUSR2)
                detected = session.agent_event("resume", before)["ns"]
                result["resume"].append({"retarget": (session.marker("pane_chat:retarget", detected) - detected) / 1e6})
        return result
    finally:
        session.close()


# ───────────── 요약 ─────────────

def flatten(sessions: list[dict]) -> dict[str, list[float]]:
    samples: dict[str, list[float]] = {}

    def add(key: str, value) -> None:
        if isinstance(value, (int, float)):
            samples.setdefault(key, []).append(float(value))
        elif isinstance(value, list):
            for item in value:
                add(key, item)
        elif isinstance(value, dict):
            for name, item in value.items():
                add(f"{key}.{name}", item)

    for result in sessions:
        for key, value in result.items():
            add(key, value)
    return samples


def summarize(values: list[float]) -> dict:
    ordered = sorted(values)
    p90_index = min(len(ordered) - 1, max(0, round(0.9 * len(ordered)) - 1))
    return {"n": len(ordered), "median_ms": round(statistics.median(ordered), 1),
            "p90_ms": round(ordered[p90_index], 1), "min_ms": round(ordered[0], 1), "max_ms": round(ordered[-1], 1)}


def write_report(output: Path, label: str, app: Path, summary: dict[str, dict]) -> None:
    lines = [f"# pane 채팅 지연 — {label}", "", f"앱: `{app}`", "",
             "| 구간 | n | 중앙값 ms | p90 ms | 최소 | 최대 |", "| --- | --- | --- | --- | --- | --- |"]
    for key, stats in summary.items():
        lines.append(f"| {key} | {stats['n']} | {stats['median_ms']} | {stats['p90_ms']} | {stats['min_ms']} | {stats['max_ms']} |")
    (output / "report.md").write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--app", required=True, type=Path)
    parser.add_argument("--runs", type=int, default=10)
    parser.add_argument("--label", required=True)
    parser.add_argument("--transcript-mb", type=float, default=4.0)
    parser.add_argument("--only", choices=["open", "connect"])
    parser.add_argument("--task", choices=sorted(TASK_TARGETS), default="tmux")
    parser.add_argument("--trust-wait", action="store_true", help="connect 세션에서 신뢰 화면 순서(trust_wait)만 잰다")
    parser.add_argument("--work", type=Path, default=Path("/var/tmp/kanvibe-pane-chat-latency"))
    args = parser.parse_args()
    output = OUTPUT_ROOT / args.label
    output.mkdir(parents=True, exist_ok=True)
    sessions: dict[str, list[dict]] = {"open": [], "connect": []}
    failures = []
    for run in range(args.runs):
        for kind in ("open", "connect"):
            if args.only and kind != args.only:
                continue
            if kind == "open" and args.task != "tmux":
                continue
            work = args.work / f"{kind}-{run}"
            shutil.rmtree(work, ignore_errors=True)
            try:
                if kind == "open":
                    sessions[kind].append(run_open_session(args.app.resolve(), work, args.transcript_mb))
                else:
                    sessions[kind].append(run_connect_session(args.app.resolve(), work, TASK_TARGETS[args.task],
                                                              args.trust_wait))
                print(f"{kind} {run}: {json.dumps(sessions[kind][-1], ensure_ascii=False)}", flush=True)
            except Exception as error:  # noqa: BLE001 - 한 회가 실패해도 나머지를 잰다
                failures.append({"kind": kind, "run": run, "error": str(error)[-400:]})
                print(f"{kind} {run} failed: {error}", flush=True)
            finally:
                logs = output / "logs" / f"{kind}-{run}"
                if (work / "logs").exists():
                    shutil.copytree(work / "logs", logs, dirs_exist_ok=True)
                shutil.copy(work / "agent-events.jsonl", logs / "agent-events.jsonl") if (work / "agent-events.jsonl").exists() else None
                shutil.rmtree(work, ignore_errors=True)
    samples = {**flatten(sessions["open"]), **flatten(sessions["connect"])}
    summary = {key: summarize(values) for key, values in samples.items()}
    (output / "result.json").write_text(json.dumps({"label": args.label, "app": str(args.app), "sessions": sessions,
                                                   "failures": failures, "summary": summary},
                                                  ensure_ascii=False, indent=2), encoding="utf-8")
    write_report(output, args.label, args.app, summary)
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    return 0 if not failures else 1


if __name__ == "__main__":
    sys.exit(main())
