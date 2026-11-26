#!/usr/bin/env python3
"""모바일 채팅 History·pane 채팅 흐름이 상대할 Rust 데스크톱.

격리 HOME에 Codex 세션 25개(각 60개 메시지), skill 셋, Hermes 세션 하나를 두고, 프로젝트 하나와 태스크
"Chat History Fixture"를 만든다. 준비가 끝나면 관측용 기기로 실제 History HTTP 계약(20개와 다음 cursor)을 확인하고,
포트를 이 실행에 맞춘 `chat-history.yaml` 사본을 `--run-dir`에 쓴다.

  --pane-chat  태스크 터미널 첫 탭에 가짜 Codex(fixture-deep 모델), 둘째 탭에 가짜 Claude를 띄운다(pane 채팅 흐름용).
  --live-cli   첫 탭에 가짜 Codex만 띄운다(skill 자동완성 흐름용).

가짜 CLI는 `parity/fixtures/agent-cli/pane-chat-cli.py`를 쓴다. 외부 모델 API를 부르지 않는다.
CLI 명령은 데스크톱 터미널 화면을 QA 제어 소켓으로 눌러 입력한다(TS fixture의 `writeTerminal`과 같은 PTY 경로).
`fixture.json`의 `maestroEnv`를 `maestro test -e`로 넘긴다.

예:
  python3 qa/mobile/prepare_chat_history_desktop.py --run-dir qa-results/mobile/pane-chat-1 --pane-chat
"""
from __future__ import annotations

import json
import re
import shlex
import sys
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from desktop_fixture import (  # noqa: E402
    MOBILE_TLS_PORT_OFFSET, REPO_ROOT, IsolatedDesktop, MobileObserver, base_argument_parser, create_repository,
    require_fresh_run_dir, run_fixture, wait_until,
)

DEFAULT_HOOK_PORT = 19738
FAKE_CLI_SOURCE = REPO_ROOT / "parity/fixtures/agent-cli/pane-chat-cli.py"
FLOW_DIRECTORY = REPO_ROOT / "mobile/.maestro/flows"
SESSION_COUNT = 25
MESSAGES_PER_SESSION = 60
HISTORY_PAGE_SIZE = 20
SKILLS = (("qa-plan", "Outline the work"), ("qa-release", "Ship a build"), ("qa-review", "Check a diff"))
CODEX_MODELS = [
    {"slug": "fixture-fast", "display_name": "Fixture Fast", "visibility": "list", "priority": 1,
     "supported_reasoning_levels": [{"effort": "low"}, {"effort": "medium"}, {"effort": "high"}],
     "service_tiers": [{"id": "priority", "name": "Fast"}]},
    {"slug": "fixture-deep", "display_name": "Fixture Deep", "visibility": "list", "priority": 2,
     "supported_reasoning_levels": [], "service_tiers": []},
]
PROMPT_A = "mobile-prompt-for-pane-a"
FIXTURE_LABEL_PATTERN = re.compile(r"^[^\W_][\w _-]{0,39}$")
CLI_READY_TIMEOUT_S = 30
SHELL_READY_WAIT_S = 3


def seed_agent_files(home: Path, repo_path: Path, history_prefix: str, hermes_title: str) -> None:
    sessions = home / ".codex" / "sessions"
    sessions.mkdir(parents=True, exist_ok=True)
    started = datetime(2026, 11, 2, tzinfo=timezone.utc)
    for index in range(SESSION_COUNT):
        session_id = f"qa-session-{index}"
        events = [{"type": "session_meta", "payload": {"id": session_id, "cwd": str(repo_path)}}]
        for message in range(MESSAGES_PER_SESSION):
            text = f"{history_prefix} session {index}" if message == 0 else f"{history_prefix} message {message}"
            timestamp = started + timedelta(minutes=index, seconds=message)
            events.append({"type": "response_item", "timestamp": timestamp.strftime("%Y-%m-%dT%H:%M:%S.000Z"),
                           "payload": {"type": "message", "role": "assistant" if message % 2 else "user",
                                       "content": [{"type": "text", "text": text}]}})
        (sessions / f"{session_id}.jsonl").write_text("\n".join(json.dumps(event) for event in events) + "\n",
                                                     encoding="utf-8")
    # 채팅 입력창의 skill 메뉴는 어느 CLI pane이든 같은 이름을 보여 주도록 두 CLI 설정 폴더에 똑같이 둔다
    for provider in (".codex", ".claude"):
        for name, description in SKILLS:
            skill_directory = home / provider / "skills" / name
            skill_directory.mkdir(parents=True, exist_ok=True)
            (skill_directory / "SKILL.md").write_text(f"---\nname: {name}\ndescription: {description}\n---\n",
                                                      encoding="utf-8")
    hermes_directory = home / ".hermes" / "sessions"
    hermes_directory.mkdir(parents=True, exist_ok=True)
    (hermes_directory / "session_qa-hermes.json").write_text(json.dumps({
        "session_id": "qa-hermes", "cwd": str(repo_path), "session_start": "2026-11-02T00:25:00.000Z",
        "last_updated": "2026-11-02T00:25:01.000Z",
        "messages": [
            {"role": "user", "content": hermes_title, "timestamp": "2026-11-02T00:25:00.000Z"},
            {"role": "assistant", "content": f"{history_prefix} Hermes reply", "timestamp": "2026-11-02T00:25:01.000Z"},
        ],
    }), encoding="utf-8")
    # 모델 설정 선택창이 읽는 Codex 모델 목록. 한 모델에만 옵션을 주어 두 경우를 모두 볼 수 있게 한다
    (home / ".codex" / "models_cache.json").write_text(json.dumps({"models": CODEX_MODELS}), encoding="utf-8")


def install_fake_clis(home: Path) -> Path:
    executable_directory = home / "fixture-bin"
    executable_directory.mkdir(parents=True, exist_ok=True)
    source = FAKE_CLI_SOURCE.read_text(encoding="utf-8").replace("__FIXTURE_HOME__", str(home))
    for provider in ("codex", "claude"):
        (executable_directory / provider).write_text(source, encoding="utf-8")
    return executable_directory


def create_history_task(desktop: IsolatedDesktop, observer: MobileObserver, repo_path: Path, history_prefix: str,
                        task_title: str) -> str:
    """모바일 등록 경로로 프로젝트를 만들고, 등록이 만든 main 루트 태스크(저장소 경로의 terminal 세션)를 쓴다.

    TS fixture는 브랜치 없는 태스크를 만들고 `connectTerminalSession`으로 터미널을 붙였다. Rust에는 그 경로가 화면에만 있고
    모바일 세션 종류 변경은 브랜치 없는 태스크를 받지 않으므로, 같은 저장소 경로에서 터미널이 도는 루트 태스크로 대신한다
    """
    observer.request("POST", "/api/mobile/projects/scan", body={"rootPath": str(repo_path.parent)})
    board = observer.board()
    project = next(entry for entry in board["projects"] if entry["repoPath"] == str(repo_path))
    tasks = [task for column in board["tasks"].values() for task in column]
    root = next(task for task in tasks if task.get("projectId") == project["id"] and task.get("branchName") == "main")
    with desktop.database() as connection:
        connection.execute("UPDATE projects SET name = ? WHERE id = ?", (history_prefix, project["id"]))
        connection.execute("UPDATE kanban_tasks SET title = ? WHERE id = ?", (task_title, root["id"]))
    return root["id"]


def verify_history_contract(observer: MobileObserver, task_id: str, run_dir: Path) -> dict:
    history = observer.request("GET", f"/api/mobile/tasks/{task_id}/ai-sessions?limit={HISTORY_PAGE_SIZE}")
    (run_dir / "http-contract.json").write_text(json.dumps(history, ensure_ascii=False, indent=2), encoding="utf-8")
    sessions = history["history"]["sessions"]
    if len(sessions) != HISTORY_PAGE_SIZE or not history["history"].get("nextCursor"):
        raise RuntimeError(f"real History contract failed: {json.dumps(history)[:600]}")
    return history


def write_chat_history_flow(run_dir: Path, task_title: str, history_prefix: str, desktop: IsolatedDesktop) -> None:
    flow = (FLOW_DIRECTORY / "chat-history.yaml").read_text(encoding="utf-8")
    flow = (flow.replace('TASK_TITLE: "Chat History Fixture"', f"TASK_TITLE: {json.dumps(task_title)}")
            .replace('HISTORY_PREFIX: "History QA"', f"HISTORY_PREFIX: {json.dumps(history_prefix)}")
            .replace('DESKTOP_PORT: "19748"', f'DESKTOP_PORT: "{desktop.mobile_tls_port}"')
            .replace("http://127.0.0.1:19738", desktop.api_url)
            .replace("file: ../scripts/issue-dev-pairing-code.js",
                     f"file: {FLOW_DIRECTORY.parent / 'scripts' / 'issue-dev-pairing-code.js'}"))
    (run_dir / "chat-history.yaml").write_text(flow, encoding="utf-8")


class TerminalDriver:
    """데스크톱 태스크 화면의 터미널에 명령을 친다. 모바일이 보는 탭·pane은 관측용 기기의 surfaces로 읽는다"""

    def __init__(self, desktop: IsolatedDesktop, observer: MobileObserver, task_id: str):
        self.desktop = desktop
        self.observer = observer
        self.task_id = task_id

    def surfaces(self) -> dict:
        return self.observer.request("GET", f"/api/mobile/tasks/{self.task_id}/surfaces")["surfaces"]

    def open_task(self) -> None:
        self.desktop.control.send({"cmd": "navigate", "route": f"/ko/task/{self.task_id}"})
        self.desktop.control.send({"cmd": "wait_for", "target": {"selector": ".xterm-screen"}, "timeout_ms": 30000})
        wait_until(lambda: self.tab_ids(), CLI_READY_TIMEOUT_S, "task terminal tab")

    def tab_ids(self) -> list[str]:
        return [tab["id"] for tab in self.surfaces().get("tabs", [])]

    def type_line(self, text: str) -> None:
        self.desktop.control.send({"cmd": "click", "target": {"selector": ".xterm-screen"}})
        self.desktop.control.send({"cmd": "type", "text": text})
        self.desktop.control.send({"cmd": "key", "keys": "Enter"})
        self.desktop.control.send({"cmd": "wait_idle"})

    def new_tab(self) -> str:
        before = set(self.tab_ids())
        self.observer.request("POST", f"/api/mobile/tasks/{self.task_id}/tabs", body={})
        created = wait_until(lambda: [tab for tab in self.tab_ids() if tab not in before], CLI_READY_TIMEOUT_S,
                             "second terminal tab")
        return created[0]


def start_fake_cli(driver: TerminalDriver, executable_directory: Path, provider: str, session_id: str) -> None:
    driver.type_line(f"/usr/bin/python3 {shlex.quote(str(executable_directory / provider))} {provider} {session_id}")


def wait_for_cli_transcript(home: Path, provider: str, session_id: str) -> None:
    if provider == "codex":
        transcript = home / ".codex" / "sessions" / f"rollout-{session_id}.jsonl"
        wait_until(transcript.exists, CLI_READY_TIMEOUT_S, f"{provider} transcript")
        return
    wait_until(lambda: list((home / ".claude" / "projects").glob(f"*/{session_id}.jsonl")), CLI_READY_TIMEOUT_S,
               f"{provider} transcript")


def prepare(desktop: IsolatedDesktop, arguments) -> tuple[dict, object]:
    run_dir = desktop.run_dir
    label = arguments.fixture_label.strip() if arguments.fixture_label else ""
    task_title = f"Chat History Fixture - {label}" if label else "Chat History Fixture"
    history_prefix = f"History QA {label}" if label else "History QA"
    hermes_title = f"{history_prefix} Hermes session"
    needs_terminal = arguments.pane_chat or arguments.live_cli
    repo_path = create_repository(run_dir / "fixture-repo" / "fixture-repo", "# Isolated chat History fixture\n",
                                  desktop.environment)
    seed_agent_files(desktop.home, repo_path, history_prefix, hermes_title)
    executable_directory = install_fake_clis(desktop.home)
    desktop.start()
    desktop.save_app_setting("background_sync_enabled", "false")
    desktop.save_app_setting("default_session_type", "terminal")
    observer = MobileObserver(desktop, "History contract probe")
    task_id = create_history_task(desktop, observer, repo_path, history_prefix, task_title)
    history = verify_history_contract(observer, task_id, run_dir)
    write_chat_history_flow(run_dir, task_title, history_prefix, desktop)
    maestro_env = {"DESKTOP_API_FROM_HOST": desktop.api_url, "DESKTOP_PORT": str(desktop.mobile_tls_port)}
    if needs_terminal:
        driver = TerminalDriver(desktop, observer, task_id)
        driver.open_task()
        first_tab = driver.tab_ids()[0]
        start_fake_cli(driver, executable_directory, "codex", "live-pane-a")
        wait_for_cli_transcript(desktop.home, "codex", "live-pane-a")
        maestro_env["FIRST_PANE_ID"] = first_tab
        if arguments.pane_chat:
            # 흐름은 첫 pane이 옵션 없는 fixture-deep 모델에서 시작한다고 본다. TS는 데스크톱 모델 선택창으로 바꿨다
            driver.type_line("/model fixture-deep")
            second_tab = driver.new_tab()
            # 모바일 API로 만든 탭은 데스크톱 화면을 다시 열어야 탭 줄에 보인다. 새 탭의 셸이 뜨기 전에 친 글자는
            # 버려지므로 기다린 뒤 둘째 탭(`Mod+Alt+2`)으로 옮겨 친다
            driver.open_task()
            time.sleep(SHELL_READY_WAIT_S)
            desktop.control.send({"cmd": "key", "keys": "Mod+Alt+2"})
            desktop.control.send({"cmd": "wait_idle"})
            start_fake_cli(driver, executable_directory, "claude", "live-pane-b")
            wait_for_cli_transcript(desktop.home, "claude", "live-pane-b")
            maestro_env.update({"SECOND_PANE_ID": second_tab, "PROMPT_A": PROMPT_A})
        time.sleep(2)
    desktop.screenshot("desktop-fixture")
    fixture = {
        "taskId": task_id, "fixtureLabel": label, "taskTitle": task_title, "historyPrefix": history_prefix,
        "hermesTitle": hermes_title, "repo": str(repo_path), "agentHome": str(desktop.home),
        "sessionCount": len(history["history"]["sessions"]), "desktopPort": desktop.hook_port,
        "mobileTlsPort": desktop.mobile_tls_port, "desktopApi": desktop.api_url, "maestroEnv": maestro_env,
        "appDataDir": str(desktop.data_dir),
    }
    return fixture, lambda: None


def main() -> None:
    parser = base_argument_parser(__doc__)
    parser.add_argument("--port", type=int, default=DEFAULT_HOOK_PORT,
                        help="hook 포트. 모바일 TLS 포트는 이 값 + 10이다")
    parser.add_argument("--fixture-label", default="", help="태스크 제목·History 접두어에 붙일 1~40자 이름")
    parser.add_argument("--pane-chat", action="store_true")
    parser.add_argument("--live-cli", action="store_true")
    arguments = parser.parse_args()
    if not 1024 <= arguments.port <= 65535 - MOBILE_TLS_PORT_OFFSET:
        parser.error(f"--port must be 1024..{65535 - MOBILE_TLS_PORT_OFFSET}")
    if arguments.fixture_label and not FIXTURE_LABEL_PATTERN.match(arguments.fixture_label.strip()):
        parser.error("--fixture-label must contain 1..40 letters, digits, spaces, underscores or hyphens")
    run_dir = require_fresh_run_dir(arguments.run_dir)
    run_fixture(lambda desktop: prepare(desktop, arguments), run_dir, Path(arguments.app), arguments.port, [])


if __name__ == "__main__":
    main()
