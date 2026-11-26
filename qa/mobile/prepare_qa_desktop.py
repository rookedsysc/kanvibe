#!/usr/bin/env python3
"""모바일 Maestro 여정이 상대할 Rust 데스크톱 한 대를 준비하고, 여정이 끝날 때까지(SIGINT/SIGTERM) 살아 있는다.

hook 서버 39736(모바일 TLS 39746)과 pane probe 19737을 고정으로 쓴다. Maestro 흐름이 이 포트를 그대로 적어 둔다.
사용자의 보드가 아니라 `--run-dir/app-data`의 격리 DB를 쓰고, tmux 세션은 이 실행만의 TMUX_TMPDIR 서버에 띄운다.

준비 순서: 데스크톱을 한 번 띄워 DB를 만들고 끈 뒤, 프로젝트·태스크·기본 세션 종류를 격리 DB에 넣고 다시 띄운다.
프로젝트와 세션 있는 태스크는 데스크톱 화면에서만 만들 수 있어 HTTP 경로가 없으므로 DB에 직접 넣는다(TS fixture와 같다).

pane probe는 기기에서 친 키가 셸에서 실제로 실행됐는지 알려 준다. 기기 화면의 터미널은 캔버스라 접근성 트리에 글자가 없고,
pty는 받은 키를 되비추므로 pane 글자로도 단정할 수 없다. 그래서 실행되어야만 생기는 파일을 본다.

예:
  python3 qa/mobile/prepare_qa_desktop.py --run-dir qa-results/mobile/journey-1
  python3 qa/mobile/prepare_qa_desktop.py --run-dir qa-results/mobile/herdr-1 --default-session-type herdr
"""
from __future__ import annotations

import sys
import time
from pathlib import Path
from typing import Callable

sys.path.insert(0, str(Path(__file__).resolve().parent))

from desktop_fixture import (  # noqa: E402
    IsolatedDesktop, ProbeServer, base_argument_parser, create_repository, git, managed_worktree_path,
    require_fresh_run_dir, run_fixture,
)

DESKTOP_HOOK_PORT = 39736
PROBE_PORT = 19737
FIXTURE_PROJECT_NAME = "kanvibe-qa-fixture"
FIXTURE_PROJECT_ID = "00000000-0000-4000-8000-000000000001"
FIXTURE_BRANCH = "maestro/fixture"
FIXTURE_NO_SESSION_TASK_ID = "00000000-0000-4000-8000-000000000002"
FIXTURE_SESSION_TASK_ID = "00000000-0000-4000-8000-000000000003"
# 흐름이 셀렉터로 그대로 쓰는 값
NO_SESSION_TASK_TITLE = "Maestro Fixture NoSession"
SESSION_TASK_TITLE = "Maestro Fixture Session"
# terminal-clipboard 흐름이 붙여넣은 값에서 찾는 글자
CLIPBOARD_FIXTURE_TEXT = "VIM_OSC52_NATIVE_OK"
# 데스크톱이 비추는 pane에 미리 찍어 두는 표식. 기기 화면에 보이면 미러링이 살아 있다
PANE_READY_MARKER = "MAESTRO_PANE_READY"
SOURCE_BRANCH_FILE = "SOURCE_BRANCH_ONLY.txt"
# 모바일에서 만든 태스크가 쓸 세션 종류. 새 설치의 기본값은 herdr라서 격리 DB에 직접 정해 둔다
DEFAULT_SESSION_TYPE = "tmux"
KNOWN_SESSION_TYPES = ("tmux", "zellij", "herdr", "terminal")
MARKER_POLL_INTERVAL_S = 0.5
TERMINAL_PROBE_TIMEOUT_S = 5


def write_clipboard_fixture(worktree_path: Path) -> None:
    """Neovim 내장 OSC 52 공급자로만 복사하므로, 기기 클립보드에 값이 오면 터미널의 OSC 52 전달이 동작한 것이다"""
    (worktree_path / "clipboard.txt").write_text(f"{CLIPBOARD_FIXTURE_TEXT}\n", encoding="utf-8")
    (worktree_path / "clipboard.vim").write_text("\n".join([
        "lua << EOF",
        "local osc52 = require('vim.ui.clipboard.osc52')",
        "vim.g.clipboard = { name = 'OSC 52', copy = { ['+'] = osc52.copy('+'), ['*'] = osc52.copy('*') }, "
        "paste = { ['+'] = function() return {} end, ['*'] = function() return {} end } }",
        "EOF",
        "set clipboard=unnamedplus",
        "",
    ]), encoding="utf-8")


def seed_board(desktop: IsolatedDesktop, repo_path: Path, worktree_path: Path, session_name: str,
               default_session_type: str) -> None:
    with desktop.database() as connection:
        connection.execute("INSERT INTO kanban_tasks (id, title) VALUES (?, ?)",
                           (FIXTURE_NO_SESSION_TASK_ID, NO_SESSION_TASK_TITLE))
        connection.execute("INSERT INTO projects (id, name, repo_path, default_branch) VALUES (?, ?, ?, 'main')",
                           (FIXTURE_PROJECT_ID, FIXTURE_PROJECT_NAME, str(repo_path)))
        connection.execute(
            "INSERT INTO kanban_tasks (id, title, branch_name, worktree_path, session_type, session_name, project_id,"
            " base_branch) VALUES (?, ?, ?, ?, 'tmux', ?, ?, 'main')",
            (FIXTURE_SESSION_TASK_ID, SESSION_TASK_TITLE, FIXTURE_BRANCH, str(worktree_path), session_name,
             FIXTURE_PROJECT_ID))
    desktop.save_app_setting("default_session_type", default_session_type)
    desktop.save_app_setting("background_sync_enabled", "false")


def prepare_worktree(desktop: IsolatedDesktop, repo_path: Path) -> Path:
    """데스크톱이 만드는 것과 같은 규칙의 worktree를 만들고, 새 태스크가 이 브랜치를 이어받는지 볼 파일을 커밋한다"""
    worktree_path = managed_worktree_path(repo_path, FIXTURE_BRANCH)
    worktree_path.parent.mkdir(parents=True, exist_ok=True)
    environment = desktop.environment
    git(repo_path, "worktree", "add", str(worktree_path), "-b", FIXTURE_BRANCH, "main", environment=environment)
    (worktree_path / SOURCE_BRANCH_FILE).write_text("maestro/fixture source branch\n", encoding="utf-8")
    git(worktree_path, "add", SOURCE_BRANCH_FILE, environment=environment)
    git(worktree_path, "commit", "-m", "chore: source branch fixture", environment=environment)
    write_clipboard_fixture(worktree_path)
    return worktree_path


def start_fixture_session(desktop: IsolatedDesktop, session_name: str, worktree_path: Path) -> None:
    """데스크톱은 tmux 세션을 터미널을 여는 시점에 만든다. 기기에서 비출 pane이 있도록 여기서 띄운다"""
    desktop.tmux("new-session", "-d", "-s", session_name, "-c", str(worktree_path))
    desktop.tmux("send-keys", "-t", session_name, f"printf '{PANE_READY_MARKER}\\n'", "Enter")


def wait_for_marker_file(worktree_path: Path, marker: str, timeout_s: float) -> bool:
    """키는 소켓을 타고 가 셸이 처리한 뒤에야 흔적을 남긴다. 아직 안 온 것을 안 올 것으로 단정하지 않게 다시 본다"""
    deadline = time.monotonic() + timeout_s
    while True:
        if marker and (worktree_path / marker).exists():
            return True
        if time.monotonic() >= deadline:
            return False
        time.sleep(MARKER_POLL_INTERVAL_S)


class PaneProbe:
    """`/command-ran`, `/terminal-state`, `/terminal-tabs`. TS fixture와 같은 질의와 응답 모양을 쓴다"""

    def __init__(self, desktop: IsolatedDesktop, repo_path: Path, session_name: str):
        self.desktop = desktop
        self.repo_path = repo_path
        self.session_name = session_name
        self.initial_pane_id = desktop.tmux("display-message", "-p", "-t", session_name, "#{pane_id}")
        self.initial_tab_ids = set(desktop.tmux("list-windows", "-t", session_name, "-F", "#{window_id}").split("\n"))

    def routes(self) -> dict[str, Callable[[str, dict[str, str]], dict]]:
        return {"/command-ran": self.command_ran, "/terminal-state": self.terminal_state,
                "/terminal-tabs": self.terminal_tabs}

    def capture_pane(self, target: str) -> str:
        try:
            return self.desktop.tmux("capture-pane", "-p", "-t", target)
        except Exception:  # noqa: BLE001 - pane이 사라졌으면 빈 화면으로 본다
            return ""

    def command_ran(self, _method: str, query: dict[str, str]) -> dict:
        marker = query.get("marker", "")
        worktree_path = managed_worktree_path(self.repo_path, query.get("branch") or FIXTURE_BRANCH)
        timeout_s = float(query.get("timeoutMs", "0")) / 1000
        return {
            "ran": wait_for_marker_file(worktree_path, marker, timeout_s),
            "sourceBranchInherited": (worktree_path / SOURCE_BRANCH_FILE).exists(),
            "pane": self.capture_pane(self.session_name),
        }

    def terminal_state(self, _method: str, query: dict[str, str]) -> dict:
        minimum = int(query.get("minColumns", "1"))
        maximum = int(query.get("maxColumns", "500"))
        deadline = time.monotonic() + TERMINAL_PROBE_TIMEOUT_S
        while True:
            size = self.desktop.tmux("display-message", "-p", "-t", self.session_name, "#{pane_width},#{pane_height}")
            columns, rows = (int(value) for value in size.split(","))
            if minimum <= columns <= maximum or time.monotonic() >= deadline:
                break
            time.sleep(MARKER_POLL_INTERVAL_S)
        return {"columns": columns, "rows": rows, "minimum": minimum, "maximum": maximum,
                "pane": self.capture_pane(self.session_name)}

    def terminal_tabs(self, _method: str, query: dict[str, str]) -> dict:
        deadline = time.monotonic() + TERMINAL_PROBE_TIMEOUT_S
        while True:
            listing = self.desktop.tmux("list-panes", "-s", "-t", self.session_name, "-F", "#{window_id}\t#{pane_id}")
            panes = [line.split("\t") for line in listing.split("\n") if line]
            created = next((pane for pane in panes if pane[0] not in self.initial_tab_ids), None)
            if created or time.monotonic() >= deadline:
                break
            time.sleep(MARKER_POLL_INTERVAL_S)
        marker = query.get("marker", "")
        worktree_path = managed_worktree_path(self.repo_path, FIXTURE_BRANCH)
        return {
            "windowCount": len({tab_id for tab_id, _ in panes}),
            "initialPaneRetained": any(pane_id == self.initial_pane_id for _, pane_id in panes),
            "newPaneId": created[1] if created else None,
            "ran": wait_for_marker_file(worktree_path, marker, TERMINAL_PROBE_TIMEOUT_S) if marker else False,
            "newPaneReceivedMarker": bool(marker and created and marker in self.capture_pane(created[1])),
            "initialPaneReceivedMarker": bool(marker and marker in self.capture_pane(self.initial_pane_id)),
        }


def prepare(desktop: IsolatedDesktop, default_session_type: str):
    run_dir = desktop.run_dir
    repo_path = create_repository(run_dir / FIXTURE_PROJECT_NAME, "KanVibe mobile QA fixture\n", desktop.environment)
    desktop.start()
    desktop.quit()
    worktree_path = prepare_worktree(desktop, repo_path)
    session_name = f"{repo_path.name}-{FIXTURE_BRANCH}".replace("/", "-")
    seed_board(desktop, repo_path, worktree_path, session_name, default_session_type)
    start_fixture_session(desktop, session_name, worktree_path)
    desktop.start()
    probe = ProbeServer(PROBE_PORT, PaneProbe(desktop, repo_path, session_name).routes(),
                        run_dir / "probe-observations.jsonl")
    desktop.screenshot("desktop-fixture")
    fixture = {
        "appDataDir": str(desktop.data_dir),
        "repoPath": str(repo_path),
        "worktreePath": str(worktree_path),
        "projectId": FIXTURE_PROJECT_ID,
        "sessionName": session_name,
        "sessionTaskId": FIXTURE_SESSION_TASK_ID,
        "noSessionTaskTitle": NO_SESSION_TASK_TITLE,
        "sessionTaskTitle": SESSION_TASK_TITLE,
        "paneReadyMarker": PANE_READY_MARKER,
        "defaultSessionType": default_session_type,
        "desktopApi": desktop.api_url,
        "mobileTlsPort": desktop.mobile_tls_port,
        "probeUrl": f"http://127.0.0.1:{PROBE_PORT}",
        "isolationRoot": str(desktop.isolation_root),
    }
    return fixture, probe.close


def main() -> None:
    parser = base_argument_parser(__doc__)
    parser.add_argument("--default-session-type", default=DEFAULT_SESSION_TYPE, choices=KNOWN_SESSION_TYPES,
                        help="모바일에서 만든 태스크의 세션 종류. 모르는 값은 데스크톱이 herdr로 읽으므로 여기서 막는다")
    arguments = parser.parse_args()
    run_dir = require_fresh_run_dir(arguments.run_dir)
    run_fixture(lambda desktop: prepare(desktop, arguments.default_session_type),
                run_dir, Path(arguments.app), DESKTOP_HOOK_PORT, [PROBE_PORT])


if __name__ == "__main__":
    main()
