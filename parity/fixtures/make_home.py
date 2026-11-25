#!/usr/bin/env python3
"""앱 프로세스에만 넘기는 격리 HOME과 가짜 CLI(bin/)를 만든다.

사용자 HOME, 셸 설정, tmux 설정, AI CLI 계정은 하나도 읽거나 쓰지 않는다.
가짜 CLI는 실제 네트워크나 계정 없이 정해진 출력만 낸다. 어떤 입력에 어떤 출력을 내는지는 각 스크립트에 적혀 있다.
"""
from __future__ import annotations

import base64
import json
import os
import stat
import struct
import zlib
from pathlib import Path

FIXTURES_DIR = Path(__file__).resolve().parent


def solid_png_data_url(width: int, height: int, rgb: tuple[int, int, int]) -> str:
    """원격 아이콘 대신 쓰는 단색 PNG를 만든다. 매번 같은 바이트가 나온다."""
    raw_rows = b"".join(b"\x00" + bytes(rgb) * width for _ in range(height))

    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw_rows, 9))
        + chunk(b"IEND", b"")
    )
    return "data:image/png;base64," + base64.b64encode(png).decode("ascii")


def write_text(path: Path, content: str, executable: bool = False) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    if executable:
        path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def set_mtime(path: Path, epoch_ms: int) -> None:
    os.utime(path, (epoch_ms / 1000, epoch_ms / 1000))


BASHRC = r"""# 스크린샷 fixture 전용 셸 설정. 대화형 셸에서만 프롬프트를 고정한다.
case $- in *i*) ;; *) return ;; esac
export PS1='\W $ '
export PROMPT_COMMAND=''
export HISTFILE=/dev/null
export LS_COLORS=''
"""

SSH_CONFIG = """# 원격 프로젝트(infra) 표시용 호스트. 연결은 즉시 거절되어 원격 조회가 빨리 실패한다.
Host devbox
  HostName 127.0.0.1
  Port 1
  BatchMode yes
  ConnectTimeout 1
  StrictHostKeyChecking no
"""

FAKE_GH = r"""#!/usr/bin/env python3
# 가짜 GitHub CLI. 네트워크 없이 fixture PR만 돌려준다.
import json, sys
args = sys.argv[1:]
PRS = {
    "feat/board-filters": {"url": "https://github.com/example/kanvibe-demo/pull/42", "state": "OPEN", "mergedAt": None, "updatedAt": "2026-01-15T02:00:00Z"},
    "fix/login-redirect": {"url": "https://github.com/example/kanvibe-demo/pull/41", "state": "OPEN", "mergedAt": None, "updatedAt": "2026-01-15T01:00:00Z"},
}
if args[:1] == ["--version"]:
    print("gh version 2.60.0 (2026-01-01)")
    sys.exit(0)
if args[:2] == ["auth", "status"]:
    print("github.com\n  Logged in to github.com account parity-fixture")
    sys.exit(0)
if args[:2] == ["pr", "list"]:
    head = args[args.index("--head") + 1] if "--head" in args else None
    print(json.dumps([PRS[head]] if head in PRS else []))
    sys.exit(0)
if args[:2] == ["pr", "view"]:
    sys.exit(1)
print("gh: unsupported in parity fixture: " + " ".join(args), file=sys.stderr)
sys.exit(1)
"""

FAKE_CODE = """#!/bin/sh
# 가짜 VS Code 실행기. 실행 중 스피너를 볼 수 있게 잠시 머문다.
sleep 20
"""

FAKE_CLAUDE = r"""#!/usr/bin/env python3
# 가짜 Claude Code CLI. 계정 상태 조회와 로그인 화면에만 답한다.
# fixture HOME은 만들 때 박아 넣는다. 앱이 넘긴 계정 경로도 그 HOME 안일 때만 쓰므로 실제 ~/.claude는 읽지 않는다.
import json, os, sys
FIXTURE_HOME = "__FIXTURE_HOME__"
def account_root(requested, default_name):
    if requested and os.path.realpath(requested).startswith(FIXTURE_HOME + os.sep):
        return requested
    return os.path.join(FIXTURE_HOME, default_name)
args = sys.argv[1:]
if args[:2] == ["auth", "status"]:
    root = account_root(os.environ.get("CLAUDE_CONFIG_DIR"), ".claude")
    logged_in = os.path.exists(os.path.join(root, ".credentials.json"))
    print(json.dumps({"loggedIn": logged_in, "email": "dev@example.com" if logged_in else None,
                      "subscriptionType": "max" if logged_in else None}))
    sys.exit(0)
if args[:1] == ["--version"]:
    print("2.0.0 (Claude Code)")
    sys.exit(0)
if args[:2] == ["auth", "login"]:
    # 로그인 진행 중 화면: 브라우저 안내를 찍고 사용자의 입력을 기다리는 것처럼 머문다
    import time
    sys.stdout.write("Opening browser to sign in to Claude...\r\n")
    sys.stdout.write("If the browser did not open, visit: https://claude.ai/oauth/authorize?code=PARITY-FIXTURE\r\n")
    sys.stdout.write("\r\nPaste code here if prompted > ")
    sys.stdout.flush()
    time.sleep(600)
    sys.exit(0)
sys.exit(1)
"""

FAKE_CODEX = r"""#!/usr/bin/env python3
# 가짜 Codex CLI. 로그인 상태 조회와 로그인 실패 화면에만 답한다.
# fixture HOME은 만들 때 박아 넣는다. 앱이 넘긴 계정 경로도 그 HOME 안일 때만 쓰므로 실제 ~/.codex는 읽지 않는다.
import os, sys
FIXTURE_HOME = "__FIXTURE_HOME__"
def account_root(requested, default_name):
    if requested and os.path.realpath(requested).startswith(FIXTURE_HOME + os.sep):
        return requested
    return os.path.join(FIXTURE_HOME, default_name)
args = sys.argv[1:]
if args[:2] == ["login", "status"]:
    root = account_root(os.environ.get("CODEX_HOME"), ".codex")
    if os.path.exists(os.path.join(root, "auth.json")):
        print("Logged in using ChatGPT")
        sys.exit(0)
    print("Not logged in")
    sys.exit(1)
if args[:1] == ["--version"]:
    print("codex-cli 0.169.0")
    sys.exit(0)
if args[:1] == ["login"]:
    # 로그인 실패 화면: 오류를 찍고 종료 코드 1로 끝난다
    sys.stdout.write("Error: failed to start the login server on port 1455 (address already in use)\r\n")
    sys.stdout.flush()
    sys.exit(1)
sys.exit(1)
"""


FAKE_OPENCODE = r"""#!/usr/bin/env python3
# 가짜 OpenCode CLI. hook 검증이 읽는 `opencode debug config`에만 답한다.
# 실제 CLI처럼 작업 디렉터리의 .opencode/plugins/kanvibe-plugin.ts가 있으면 등록된 플러그인으로 알려준다.
import json, os, pathlib, sys
args = sys.argv[1:]
if args[:2] == ["debug", "config"]:
    plugin = pathlib.Path(os.getcwd()) / ".opencode" / "plugins" / "kanvibe-plugin.ts"
    print(json.dumps({"plugin": [plugin.as_uri()] if plugin.exists() else []}))
    sys.exit(0)
if args[:1] == ["--version"]:
    print("1.0.0")
    sys.exit(0)
sys.exit(1)
"""


def bake_fixture_home(script: str, home_dir: Path) -> str:
    """가짜 CLI가 환경 변수나 실제 HOME 대신 이 fixture HOME만 보도록 경로를 박아 넣는다."""
    return script.replace("__FIXTURE_HOME__", str(home_dir.resolve()))


def build_fake_bin(bin_dir: Path, home_dir: Path) -> None:
    write_text(bin_dir / "opencode", FAKE_OPENCODE, executable=True)
    write_text(bin_dir / "gh", FAKE_GH, executable=True)
    write_text(bin_dir / "code", FAKE_CODE, executable=True)
    write_text(bin_dir / "claude", bake_fixture_home(FAKE_CLAUDE, home_dir), executable=True)
    write_text(bin_dir / "codex", bake_fixture_home(FAKE_CODEX, home_dir), executable=True)


def claude_project_directory_name(target_path: str) -> str:
    return target_path.replace("/", "-").replace("_", "-")


def build_home(fixture_root: Path, home_dir: Path, fixed_now_epoch_ms: int, extras: list[str]) -> None:
    """fixture_root·home_dir는 호출자가 넘긴 경로만 쓴다. 실제 HOME이나 환경 변수의 계정 경로는 읽지 않는다."""
    home_dir.mkdir(parents=True, exist_ok=True)
    write_text(home_dir / ".bashrc", BASHRC)
    write_text(home_dir / ".bash_profile", "[ -f ~/.bashrc ] && . ~/.bashrc\n")
    write_text(home_dir / ".profile", "[ -f ~/.bashrc ] && . ~/.bashrc\n")
    write_text(home_dir / ".gitconfig", "[user]\n\tname = Parity Fixture\n\temail = parity@example.invalid\n[init]\n\tdefaultBranch = main\n")
    write_text(home_dir / ".ssh" / "config", SSH_CONFIG)
    (home_dir / ".ssh").chmod(0o700)
    (home_dir / ".ssh" / "config").chmod(0o600)
    for runtime_dir in ("runtime", "tmux", "config", "cache", "data", "state"):
        (fixture_root / "xdg" / runtime_dir).mkdir(parents=True, exist_ok=True)
    (fixture_root / "xdg" / "runtime").chmod(0o700)
    (fixture_root / "xdg" / "tmux").chmod(0o700)
    build_fake_bin(fixture_root / "bin", home_dir)
    for extra in extras:
        EXTRAS[extra](fixture_root, home_dir, fixed_now_epoch_ms)


def write_json_lines(path: Path, records: list[dict], mtime_epoch_ms: int) -> None:
    write_text(path, "".join(json.dumps(record, ensure_ascii=False) + "\n" for record in records))
    set_mtime(path, mtime_epoch_ms)


def board_filters_worktree(fixture_root: Path) -> str:
    return str(fixture_root / "repos" / "kanvibe-demo__worktrees" / "feat-board-filters")


CHAT_ASSISTANT_MARKDOWN = "\n".join([
    "필터 상태는 앱 설정 대신 URL query에 두었습니다.",
    "",
    "## 변경 요약",
    "",
    "- `src/filters.ts`에 `applyFilters`를 추가했습니다.",
    "- **보드 헤더**의 프로젝트 선택과 같은 값을 씁니다.",
    "",
    "```ts",
    "export const onlyOpen: CardFilter = (card) => card.status !== \"done\";",
    "```",
    "",
    "| 필터 | 저장 위치 |",
    "| --- | --- |",
    "| 프로젝트 | URL |",
    "| 상태 | URL |",
    "",
    "```mermaid",
    "graph TD",
    "  A[Board header] --> B[URL query]",
    "  B --> C[Filtered cards]",
    "```",
])


def build_chat_history(fixture_root: Path, home_dir: Path, fixed_now_epoch_ms: int) -> None:
    """작업 상세의 AI 대화 기록 화면용 Claude·Codex 세션 기록. 시각은 FIXED_NOW 기준으로 고정한다."""
    worktree_path = board_filters_worktree(fixture_root)
    session_id = "parity-claude-board-filters"
    claude_file = home_dir / ".claude" / "projects" / claude_project_directory_name(worktree_path) / f"{session_id}.jsonl"
    base = {"sessionId": session_id, "cwd": worktree_path}
    write_json_lines(claude_file, [
        {**base, "type": "user", "timestamp": "2026-01-15T02:40:00.000Z",
         "message": {"role": "user", "content": [{"type": "text", "text": "보드 헤더에 프로젝트·상태 필터를 추가하고 선택을 기억해 줘."}]}},
        {**base, "type": "assistant", "timestamp": "2026-01-15T02:41:00.000Z",
         "message": {"role": "assistant", "model": "claude-opus-5-5", "content": [
             {"type": "tool_use", "id": "toolu_parity_1", "name": "Read", "input": {"file_path": f"{worktree_path}/src/board.ts"}}]}},
        {**base, "type": "user", "timestamp": "2026-01-15T02:41:10.000Z",
         "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_parity_1", "content": "export interface Card { ... }"}]}},
        {**base, "type": "assistant", "timestamp": "2026-01-15T02:43:00.000Z",
         "message": {"role": "assistant", "model": "claude-opus-5-5", "content": [
             {"type": "tool_use", "id": "toolu_parity_2", "name": "Write",
              "input": {"file_path": f"{worktree_path}/src/filters.ts", "content": "export type CardFilter = (card: Card) => boolean;"}}]}},
        {**base, "type": "user", "timestamp": "2026-01-15T02:43:05.000Z",
         "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_parity_2", "content": "File created"}]}},
        {**base, "type": "assistant", "timestamp": "2026-01-15T02:45:00.000Z",
         "message": {"role": "assistant", "model": "claude-opus-5-5", "content": [{"type": "text", "text": CHAT_ASSISTANT_MARKDOWN}]}},
    ], fixed_now_epoch_ms - 15 * 60_000)

    codex_file = home_dir / ".codex" / "sessions" / "2026" / "01" / "15" / "rollout-parity-codex-board-filters.jsonl"
    write_json_lines(codex_file, [
        {"type": "session_meta", "timestamp": "2026-01-15T01:00:00.000Z",
         "payload": {"id": "parity-codex-board-filters", "cwd": worktree_path, "timestamp": "2026-01-15T01:00:00.000Z"}},
        {"type": "response_item", "timestamp": "2026-01-15T01:01:00.000Z",
         "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "필터 테스트를 먼저 작성해 줘."}]}},
        {"type": "response_item", "timestamp": "2026-01-15T01:03:00.000Z",
         "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "`applyFilters` 테스트 3개를 추가했습니다."}]}},
    ], fixed_now_epoch_ms - 120 * 60_000)


def build_live_sessions(fixture_root: Path, home_dir: Path, fixed_now_epoch_ms: int) -> None:
    """실행 중 세션 패널·호출 그래프용 기록. 수정 시각이 90초 창 안에 들도록 FIXED_NOW 직전으로 둔다."""
    worktree_path = board_filters_worktree(fixture_root)
    project_dir = home_dir / ".claude" / "projects" / claude_project_directory_name(worktree_path)
    session_id = "parity-live-claude"
    write_json_lines(project_dir / f"{session_id}.jsonl", [
        {"type": "user", "sessionId": session_id, "cwd": worktree_path, "timestamp": "2026-01-15T02:58:00.000Z",
         "message": {"role": "user", "content": [{"type": "text", "text": "보드 필터 구현을 이어서 진행해 줘."}]}},
    ], fixed_now_epoch_ms - 2_000)
    write_json_lines(project_dir / session_id / "subagents" / "agent-parity-running.jsonl", [
        {"agentId": "parity-running", "isSidechain": True, "timestamp": "2026-01-15T02:58:30.000Z",
         "message": {"role": "user", "content": [{"type": "text", "text": "서브에이전트: 필터 사용처 조사"}]}},
    ], fixed_now_epoch_ms - 3_000)
    write_json_lines(project_dir / session_id / "subagents" / "agent-parity-finished.jsonl", [
        {"agentId": "parity-finished", "isSidechain": True, "timestamp": "2026-01-15T02:50:00.000Z",
         "message": {"role": "user", "content": [{"type": "text", "text": "서브에이전트: 테스트 실행"}]}},
    ], fixed_now_epoch_ms - 600_000)


def build_ai_accounts(fixture_root: Path, home_dir: Path, fixed_now_epoch_ms: int) -> None:
    """AI 계정 화면용 자격증명 파일. 토큰은 가짜이며 네트워크 응답은 network/default.json이 돌려준다."""
    far_future_ms = fixed_now_epoch_ms + 30 * 86_400_000
    write_text(home_dir / ".claude" / ".credentials.json", json.dumps({"claudeAiOauth": {
        "accessToken": "parity-claude-access-token", "refreshToken": "parity-claude-refresh-token",
        "expiresAt": far_future_ms, "scopes": ["user:inference", "user:profile"], "subscriptionType": "max"}}))
    (home_dir / ".claude-work").mkdir(parents=True, exist_ok=True)
    write_text(home_dir / ".codex" / "auth.json", json.dumps({"OPENAI_API_KEY": None, "tokens": {
        "access_token": "parity-codex-access-token", "account_id": "parity-codex-account"}}))


def build_pane_chat_agent(fixture_root: Path, home_dir: Path, fixed_now_epoch_ms: int) -> None:
    """Pane 채팅 화면용 가짜 CLI를 <root>/agent-bin/claude·codex로 둔다. PATH에는 넣지 않고 단계에서 직접 실행한다."""
    source = bake_fixture_home((FIXTURES_DIR / "agent-cli" / "pane-chat-cli.py").read_text(encoding="utf-8"), home_dir)
    for provider in ("claude", "codex"):
        write_text(fixture_root / "agent-bin" / provider, source, executable=True)


EXTRAS: dict = {
    "pane-chat-agent": build_pane_chat_agent,
    "chat-history": build_chat_history,
    "live-sessions": build_live_sessions,
    "ai-accounts": build_ai_accounts,
}
