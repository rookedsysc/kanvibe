#!/usr/bin/env python3
"""릴리스 `kanvibe`(QA 모드 아님)가 새 데이터 디렉터리로 실제 앱처럼 뜨고 깨끗하게 끝나는지 확인한다.

확인하는 것:
  1. 빈 임시 데이터 디렉터리에서 DB를 만들고 마이그레이션을 모두 적용한다(`migrations` 표, 핵심 표).
  2. 프로세스 안 hook 서버(`/api/hooks/health` 200)와 모바일 TLS 포트(hook 포트 + 10)가 듣는다.
  3. 보드 창이 뜬다(WM_CLASS `kanvibe` 창, 화면 캡처가 빈 화면이 아님).
  4. 진단 로그(`<data>/logs/kanvibe-desktop.log`)에 시작·서버 기록이 남는다.
  5. SIGTERM을 받으면 정상 종료 경로로 끝나고(종료 코드 0) 두 포트가 닫힌다.

사용자 환경은 건드리지 않는다. 앱에는 격리 HOME·XDG_*·TMUX_TMPDIR과 Xvfb 화면만 넘기고, hook 포트는
`--kanvibe-hook-port`로 비어 있는 포트를 준다(설치된 앱의 9736과 겹치지 않게).

예:
  cargo build --release -p kanvibe-app --bin kanvibe
  python3 parity/tools/app-launch/check_release_launch.py
"""
from __future__ import annotations

import argparse
import http.client
import json
import shutil
import signal
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
DEFAULT_APP = REPO_ROOT / "target/release/kanvibe"
MOBILE_TLS_PORT_OFFSET = 10
EXPECTED_TABLES = {"projects", "kanban_tasks", "app_settings", "migrations"}
WINDOW_SIZE = (1600, 1000)


def free_hook_port() -> int:
    """평문 포트와 TLS 포트(+10)가 모두 비어 있는 포트"""
    for _ in range(200):
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]
        if port + MOBILE_TLS_PORT_OFFSET > 65535:
            continue
        with socket.socket() as tls_probe:
            try:
                tls_probe.bind(("0.0.0.0", port + MOBILE_TLS_PORT_OFFSET))
            except OSError:
                continue
        return port
    raise RuntimeError("no free hook port pair")


def free_display() -> int:
    for number in range(140, 200):
        if not Path(f"/tmp/.X{number}-lock").exists() and not Path(f"/tmp/.X11-unix/X{number}").exists():
            return number
    raise RuntimeError("no free X display")


def start_xvfb(number: int, log_path: Path) -> subprocess.Popen:
    width, height = WINDOW_SIZE
    process = subprocess.Popen(
        ["Xvfb", f":{number}", "-screen", "0", f"{width}x{height}x24", "-nolisten", "tcp", "-dpi", "96"],
        stdout=log_path.open("w"), stderr=subprocess.STDOUT,
    )
    wait_until(lambda: Path(f"/tmp/.X11-unix/X{number}").exists(), 10, "Xvfb socket")
    return process


def wait_until(predicate, timeout_s: float, what: str):
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.1)
    raise RuntimeError(f"timed out waiting for {what}")


def app_environment(root: Path, display: int) -> dict[str, str]:
    """앱 프로세스 전용 좁은 환경. 사용자 셸 환경을 넘기지 않는다"""
    xdg = root / "xdg"
    home = root / "home"
    for directory in (home, xdg / "config", xdg / "cache", xdg / "data", xdg / "state", xdg / "runtime", xdg / "tmux"):
        directory.mkdir(parents=True, exist_ok=True)
    (xdg / "runtime").chmod(0o700)
    (xdg / "tmux").chmod(0o700)
    return {
        "PATH": "/usr/local/bin:/usr/bin:/bin",
        "HOME": str(home),
        "USER": "kanvibe-launch",
        "LOGNAME": "kanvibe-launch",
        "SHELL": "/bin/bash",
        "TERM": "xterm-256color",
        "LANG": "en_US.UTF-8",
        "TZ": "UTC",
        "DISPLAY": f":{display}",
        "XDG_RUNTIME_DIR": str(xdg / "runtime"),
        "XDG_CONFIG_HOME": str(xdg / "config"),
        "XDG_CACHE_HOME": str(xdg / "cache"),
        "XDG_DATA_HOME": str(xdg / "data"),
        "XDG_STATE_HOME": str(xdg / "state"),
        "TMUX_TMPDIR": str(xdg / "tmux"),
    }


def health_status(port: int) -> int | None:
    try:
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=2)
        connection.request("GET", "/api/hooks/health")
        status = connection.getresponse().status
        connection.close()
        return status
    except OSError:
        return None


def is_listening(port: int) -> bool:
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=1):
            return True
    except OSError:
        return False


def database_report(database_path: Path) -> dict:
    connection = sqlite3.connect(f"file:{database_path}?mode=ro", uri=True)
    try:
        tables = {row[0] for row in connection.execute("SELECT name FROM sqlite_master WHERE type = 'table'")}
        migrations = connection.execute("SELECT COUNT(*) FROM migrations").fetchone()[0]
    finally:
        connection.close()
    return {"tables": sorted(tables), "migrations": migrations}


def board_window_ids(environment: dict[str, str]) -> list[str]:
    result = subprocess.run(["xdotool", "search", "--onlyvisible", "--class", "kanvibe"],
                            env=environment, capture_output=True, text=True)
    return result.stdout.split()


def capture_screen(display: int, output_path: Path) -> int:
    """화면을 찍고 서로 다른 색 수를 돌려준다. 빈 화면이면 1이다"""
    width, height = WINDOW_SIZE
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "x11grab", "-draw_mouse", "0",
                    "-video_size", f"{width}x{height}", "-i", f":{display}.0+0,0",
                    "-frames:v", "1", "-pix_fmt", "rgb24", str(output_path)], check=True)
    raw = subprocess.run(["ffmpeg", "-loglevel", "error", "-i", str(output_path), "-f", "rawvideo",
                          "-pix_fmt", "rgb24", "-"], check=True, capture_output=True).stdout
    return len({raw[index:index + 3] for index in range(0, len(raw), 3 * 7)})


def stop_process(process: subprocess.Popen | None) -> None:
    if process is None or process.poll() is not None:
        return
    process.kill()
    process.wait()


def run(app: Path, root: Path, settle_s: float) -> dict:
    data_dir = root / "appdata"
    display = free_display()
    port = free_hook_port()
    tls_port = port + MOBILE_TLS_PORT_OFFSET
    environment = app_environment(root, display)
    logs = root / "logs"
    logs.mkdir(parents=True, exist_ok=True)
    report: dict = {"app": str(app), "data_dir": str(data_dir), "hook_port": port, "tls_port": tls_port,
                    "display": display, "checks": {}}
    checks = report["checks"]
    xvfb = start_xvfb(display, logs / "xvfb.log")
    app_process = None
    app_log = (logs / "app.log").open("w")
    try:
        started = time.monotonic()
        app_process = subprocess.Popen([str(app), "--data-dir", str(data_dir), "--kanvibe-hook-port", str(port)],
                                       env=environment, stdout=app_log, stderr=subprocess.STDOUT)
        wait_until(lambda: health_status(port) == 200 or app_process.poll() is not None, 60, "hook server")
        if app_process.poll() is not None:
            raise RuntimeError(f"app exited early with {app_process.returncode}")
        report["server_ready_s"] = round(time.monotonic() - started, 2)
        checks["hook_health_200"] = health_status(port) == 200
        checks["tls_port_listening"] = wait_until(lambda: is_listening(tls_port), 30, "mobile TLS port")
        windows = wait_until(lambda: board_window_ids(environment), 30, "board window")
        report["window_ids"] = windows
        database_path = data_dir / "kanvibe.db"
        checks["database_created"] = database_path.exists()
        database = database_report(database_path)
        report["database"] = database
        checks["migrations_applied"] = database["migrations"] >= 17 and EXPECTED_TABLES <= set(database["tables"])
        time.sleep(settle_s)
        distinct_colors = capture_screen(display, root / "board.png")
        report["screenshot"] = str(root / "board.png")
        report["screenshot_distinct_colors"] = distinct_colors
        checks["board_rendered"] = distinct_colors > 20
        diagnostics_log = data_dir / "logs" / "kanvibe-desktop.log"
        diagnostics = diagnostics_log.read_text(encoding="utf-8") if diagnostics_log.exists() else ""
        checks["diagnostics_logged"] = "main:startup" in diagnostics and "main:hook-server-listening" in diagnostics
        checks["tls_certificate_created"] = (data_dir / "mobile-tls" / "certificate.pem").exists()

        quit_started = time.monotonic()
        app_process.send_signal(signal.SIGTERM)
        try:
            exit_code = app_process.wait(timeout=20)
        except subprocess.TimeoutExpired:
            exit_code = None
        report["exit_code"] = exit_code
        report["quit_s"] = round(time.monotonic() - quit_started, 2)
        checks["clean_exit"] = exit_code == 0
        checks["ports_closed"] = not is_listening(port) and not is_listening(tls_port)
    finally:
        stop_process(app_process)
        app_log.close()
        tmux_environment = {"PATH": environment["PATH"], "HOME": environment["HOME"],
                            "TMUX_TMPDIR": environment["TMUX_TMPDIR"]}
        subprocess.run(["tmux", "kill-server"], env=tmux_environment, capture_output=True)
        xvfb.terminate()
        xvfb.wait(timeout=5)
    report["passed"] = all(checks.values())
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--app", type=Path, default=DEFAULT_APP)
    parser.add_argument("--root", type=Path, help="작업 디렉터리(기본: 새 임시 디렉터리, 성공하면 지운다)")
    parser.add_argument("--settle", type=float, default=3.0, help="캡처 전 기다릴 초")
    args = parser.parse_args()
    if not args.app.exists():
        print(f"app binary not found: {args.app}", file=sys.stderr)
        return 2
    root = args.root or Path(tempfile.mkdtemp(prefix="kanvibe-release-launch-"))
    root.mkdir(parents=True, exist_ok=True)
    try:
        report = run(args.app.resolve(), root, args.settle)
    except Exception as error:  # 실패 원인과 로그 위치를 남기고 끝낸다
        report = {"passed": False, "error": str(error), "root": str(root)}
    report["root"] = str(root)
    print(json.dumps(report, indent=2, ensure_ascii=False))
    if report["passed"] and args.root is None:
        shutil.rmtree(root, ignore_errors=True)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
