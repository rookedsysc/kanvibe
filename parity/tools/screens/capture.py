"""앱 실행 공용 부품: fixture, 고정 글꼴 설정, Xvfb, 격리 앱 환경, ffmpeg x11grab.

`perf/run_perf.py`와 `perf/pane_chat_latency.py`가 가져다 쓴다. TS 앱과 화면을 비교하던 도구와 기준 이미지는
커밋 9a82617e까지 남아 있다.

창 관리자 없이 Xvfb 화면 크기를 창 크기와 같게 두고, 창은 (0,0)에 정확한 크기로 놓는다.
사용자 tmux·셸·HOME은 건드리지 않는다. 앱 프로세스에만 격리 HOME, XDG_*, TMUX_TMPDIR, FONTCONFIG_FILE을 넘긴다.
"""
from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parents[2]
PARITY_DIR = REPO_ROOT / "parity"
FIXTURES_DIR = PARITY_DIR / "fixtures"
sys.path.insert(0, str(FIXTURES_DIR))
sys.path.insert(0, str(TOOLS_DIR))

from build_fixture import FIXED_NOW_EPOCH_MS, build_fixture  # noqa: E402,F401  (가져다 쓰는 쪽에 다시 내보낸다)
from fontconfig import write_pinned_fontconfig  # noqa: E402,F401


def multiplexer_bin_dirs() -> list[str]:
    """tmux·zellij·herdr가 깔린 디렉터리만 앱 PATH에 더한다. 머신마다 위치가 다르므로 실행 시 찾는다."""
    dirs = []
    for tool in ("tmux", "zellij", "herdr"):
        found = shutil.which(tool)
        if found and os.path.dirname(found) not in dirs:
            dirs.append(os.path.dirname(found))
    return dirs


def start_xvfb(display_number: int, width: int, height: int, log_path: Path) -> subprocess.Popen:
    lock_file = Path(f"/tmp/.X{display_number}-lock")
    if lock_file.exists():
        raise RuntimeError(f"display :{display_number} is in use ({lock_file})")
    log_handle = log_path.open("w")
    process = subprocess.Popen(
        ["Xvfb", f":{display_number}", "-screen", "0", f"{width}x{height}x24", "-nolisten", "tcp", "-dpi", "96"],
        stdout=log_handle, stderr=subprocess.STDOUT,
    )
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if Path(f"/tmp/.X11-unix/X{display_number}").exists():
            return process
        time.sleep(0.05)
    process.kill()
    raise RuntimeError("Xvfb did not start")


def stop_process(process: subprocess.Popen | None) -> None:
    if process is None or process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()


def build_app_environment(fixture: dict, fixture_root: Path, display_number: int, fontconfig_path: Path) -> dict[str, str]:
    """앱 프로세스 전용 환경. 사용자 셸 환경을 통째로 넘기지 않고 필요한 값만 고른다."""
    xdg = fixture_root / "xdg"
    return {
        "PATH": ":".join([fixture["bin"], *multiplexer_bin_dirs(), "/usr/local/bin", "/usr/bin", "/bin"]),
        "HOME": fixture["home"],
        "USER": "parity",
        "LOGNAME": "parity",
        "SHELL": "/bin/bash",
        "TERM": "xterm-256color",
        "LANG": "en_US.UTF-8",
        "LC_ALL": "en_US.UTF-8",
        "TZ": "UTC",
        "DISPLAY": f":{display_number}",
        "XDG_RUNTIME_DIR": str(xdg / "runtime"),
        "XDG_CONFIG_HOME": str(xdg / "config"),
        "XDG_CACHE_HOME": str(xdg / "cache"),
        "XDG_DATA_HOME": str(xdg / "data"),
        "XDG_STATE_HOME": str(xdg / "state"),
        "TMUX_TMPDIR": str(xdg / "tmux"),
        "FONTCONFIG_FILE": str(fontconfig_path),
        "KANVIBE_QA_MODE": "1",
        "KANVIBE_APP_DATA_DIR": fixture["appdata"],
        "KANVIBE_DB_PATH": fixture["database"],
        "CI": "1",
    }


def kill_fixture_multiplexers(environment: dict[str, str]) -> None:
    """이 실행의 TMUX_TMPDIR에 있는 tmux 서버와 격리 XDG_CONFIG_HOME에서 실행 중인 Herdr 세션만 끈다.

    환경을 명시하므로 HERDR_* 변수가 섞이지 않아 사용자 기본 소켓과 Herdr 서버는 보이지 않는다. Herdr는 세션 폴더가 격리
    XDG_CONFIG_HOME 안에 있는 세션만 이름으로 멈추고 지우며 `server stop`은 쓰지 않는다.
    """
    tmux_environment = {"PATH": environment["PATH"], "HOME": environment["HOME"], "TMUX_TMPDIR": environment["TMUX_TMPDIR"]}
    subprocess.run(["tmux", "kill-server"], env=tmux_environment, capture_output=True)
    herdr = shutil.which("herdr", path=environment["PATH"])
    if herdr is None:
        return
    herdr_environment = {"PATH": environment["PATH"], "HOME": environment["HOME"],
                         "XDG_CONFIG_HOME": environment["XDG_CONFIG_HOME"]}
    listed = subprocess.run([herdr, "session", "list", "--json"], env=herdr_environment, capture_output=True, text=True)
    try:
        sessions = json.loads(listed.stdout)["sessions"]
    except (ValueError, KeyError, TypeError):
        return
    for session in sessions:
        is_fixture_session = str(session.get("session_dir", "")).startswith(environment["XDG_CONFIG_HOME"] + "/")
        if session.get("running") and is_fixture_session:
            subprocess.run([herdr, "session", "stop", session["name"]], env=herdr_environment, capture_output=True)
            subprocess.run([herdr, "session", "delete", session["name"]], env=herdr_environment, capture_output=True)


def kill_leftover_processes(fixture_root: Path) -> None:
    """fixture 경로를 명령줄에 가진 남은 프로세스(셸, 가짜 CLI)를 정리한다."""
    result = subprocess.run(["pgrep", "-f", str(fixture_root)], capture_output=True, text=True)
    for pid_text in result.stdout.split():
        pid = int(pid_text)
        if pid == os.getpid():
            continue
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass


def grab_window(display_number: int, width: int, height: int, output_path: Path) -> None:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-y", "-f", "x11grab", "-draw_mouse", "0",
         "-video_size", f"{width}x{height}", "-i", f":{display_number}.0+0,0",
         "-frames:v", "1", "-pix_fmt", "rgb24", str(output_path)],
        check=True,
    )
