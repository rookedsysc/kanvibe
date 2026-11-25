"""재생기가 띄우는 서버의 격리 실행 환경. 골든을 녹화할 때 TS 앱에 준 것과 같은 env를 만든다.

격리 원칙(CLAUDE.md 환경 안전 규칙):
- 자식 프로세스 env는 부모 env를 통째로 넘기지 않고 필요한 값만 골라 새로 만든다. 부모의 TMUX/ZELLIJ/KANVIBE_* 값은 넘기지 않는다.
- HOME, XDG_*, TMUX_TMPDIR를 실행 디렉터리 안으로 돌려 사용자의 tmux 서버, 셸 설정, CLI 계정 파일에 닿지 않게 한다.
- 이 값들은 띄운 자식에게만 적용하고 재생기 자신의 os.environ은 바꾸지 않는다.
"""

from __future__ import annotations

import os
import socket
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[4]
MOBILE_TLS_PORT_OFFSET = 10

PARENT_ENV_PASSTHROUGH = ("LANG", "LC_ALL", "USER", "LOGNAME")

# 골든이 기계마다 같게 나오도록 자식 PATH를 시스템 디렉터리로 좁힌다. 사용자 디렉터리(~/.local/bin, linuxbrew)의
# claude·codex·gemini·opencode·herdr·zellij는 보이지 않고 git·tmux·gh만 시스템에서 찾는다. README의 "Environment contract" 참고
DEFAULT_CHILD_PATH = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"


def build_child_environment(run_dir: Path, socket_dir: Path, child_path: str = DEFAULT_CHILD_PATH) -> dict[str, str]:
    """띄울 앱에만 줄 env. 부모 값은 로캘·사용자 이름만 옮긴다.

    tmux·herdr 소켓은 Unix 소켓 경로 길이(108바이트) 한도에 걸리지 않도록 짧은 [socket_dir] 아래에 둔다.
    """
    home = run_dir / "home"
    runtime_dir = socket_dir / "run"
    tmux_dir = socket_dir / "tmux"
    app_data = run_dir / "appdata"
    for directory in (home / ".config", home / ".local" / "share", home / ".cache", home / ".local" / "state", runtime_dir, tmux_dir, app_data):
        directory.mkdir(parents=True, exist_ok=True)
    runtime_dir.chmod(0o700)
    tmux_dir.chmod(0o700)
    # 셸 프롬프트를 고정해 pane 출력이 사용자 셸 설정에 흔들리지 않게 한다
    # tmux는 로그인 셸을 띄우므로 ~/.bash_profile이 읽힌다. 시스템 profile의 프롬프트 훅도 끈다
    (home / ".bash_profile").write_text("PS1='$ '\nPROMPT_COMMAND=''\nunset PS0\n")
    (home / ".gitconfig").write_text("[user]\n\tname = KanVibe Contract\n\temail = contract@kanvibe.invalid\n[init]\n\tdefaultBranch = main\n")

    environment = {name: os.environ[name] for name in PARENT_ENV_PASSTHROUGH if name in os.environ}
    environment.setdefault("LANG", "C.UTF-8")
    environment.update({
        "PATH": child_path,
        "HOME": str(home),
        "SHELL": "/bin/bash",
        "TERM": "xterm-256color",
        "XDG_CONFIG_HOME": str(home / ".config"),
        "XDG_DATA_HOME": str(home / ".local" / "share"),
        "XDG_CACHE_HOME": str(home / ".cache"),
        "XDG_STATE_HOME": str(home / ".local" / "state"),
        "XDG_RUNTIME_DIR": str(runtime_dir),
        "TMUX_TMPDIR": str(tmux_dir),
        "CI": "1",
        "KANVIBE_QA_MODE": "1",
        "KANVIBE_APP_DATA_DIR": str(app_data),
        "KANVIBE_DB_PATH": str(app_data / "kanvibe.db"),
    })
    return environment


def is_port_free(port: int) -> bool:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        # 서버도 SO_REUSEADDR로 listen하므로, 직전 실행의 TIME_WAIT 연결은 포트 사용 중으로 보지 않는다
        probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            probe.bind(("127.0.0.1", port))
        except OSError:
            return False
    return True
