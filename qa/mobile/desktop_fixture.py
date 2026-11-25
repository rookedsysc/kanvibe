"""모바일 Maestro 흐름이 상대할 Rust 데스크톱(`kanvibe`)을 격리해 띄우는 공용 부품. 표준 라이브러리만 쓴다.

데스크톱은 자동 QA 모드(`AUTOMATED_QA_FLAG`)와 `--data-dir`, `--kanvibe-hook-port`로 띄운다. 이 모드는 원격 namespace를
데이터 디렉터리마다 따로 잡고, 모바일 E2E가 쓰는 `/api/mobile/dev/pairing-code`를 열어 둔다(TS 소스 실행과 같다).

사용자 환경은 건드리지 않는다. 앱과 fixture가 띄우는 tmux·herdr·셸에는 짧은 임시 디렉터리 아래의 HOME·XDG_*·TMUX_TMPDIR과
이 실행만의 Xvfb 화면을 넘기고, 이 프로세스의 환경은 바꾸지 않는다. 임시 디렉터리는 herdr Unix socket 경로 상한 때문에
`--run-dir`과 따로 짧게 만든다. 증거(로그, fixture.json, 관측 기록, 데스크톱 화면)는 `--run-dir`에 남는다.

데스크톱 화면에서만 할 수 있는 일은 QA 제어 소켓(`--control-socket`, 한 줄 JSON)으로 하고, 모바일이 보는 상태는 관측용
기기를 하나 더 페어링해 실제 모바일 API로 읽는다.
"""
from __future__ import annotations

import argparse
import http.client
import json
import os
import shutil
import signal
import socket
import sqlite3
import subprocess
import tempfile
import threading
import time
import uuid
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Callable
from urllib.parse import parse_qs, urlparse

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_APP = REPO_ROOT / "target/release/kanvibe"
# 자동 QA 실행 스위치. 앱에서 이름이 바뀌면 여기 한 곳만 고친다
AUTOMATED_QA_FLAG = "--qa-harness"
# `kanvibe-server` `MOBILE_TLS_PORT_OFFSET`: 모바일 TLS 포트는 hook 포트 + 10이다
MOBILE_TLS_PORT_OFFSET = 10
SCREEN_SIZE = (1440, 960)
APP_READY_TIMEOUT_S = 90
APP_QUIT_TIMEOUT_S = 15
POLL_INTERVAL_S = 0.2
# 앱이 찾아 써야 하는 도구. 머신마다 위치가 달라 이 프로세스의 PATH에서 찾아 디렉터리만 앱 PATH에 더한다
APP_TOOLS = ("git", "tmux", "herdr", "zellij", "python3")


def wait_until(predicate: Callable[[], object], timeout_s: float, what: str) -> object:
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(POLL_INTERVAL_S)
    raise RuntimeError(f"timed out waiting for {what}")


def assert_port_free(port: int) -> None:
    """고정 포트를 다른 실행이 쥐고 있으면 기기가 엉뚱한 데스크톱에 붙으므로 시작하지 않는다"""
    with socket.socket() as probe:
        probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            probe.bind(("0.0.0.0", port))
        except OSError as error:
            raise RuntimeError(f"port {port} is already in use") from error


def free_display() -> int:
    for number in range(150, 220):
        if not Path(f"/tmp/.X{number}-lock").exists() and not Path(f"/tmp/.X11-unix/X{number}").exists():
            return number
    raise RuntimeError("no free X display")


def require_fresh_run_dir(raw_path: str) -> Path:
    """앞선 실행의 증거를 덮지 않고, 남은 DB가 상태를 흔들지 않게 새 디렉터리만 받는다"""
    run_dir = Path(raw_path).resolve()
    if run_dir.exists() and any(run_dir.iterdir()):
        raise RuntimeError(f"use a fresh --run-dir: {run_dir}")
    run_dir.mkdir(parents=True, exist_ok=True)
    return run_dir


def base_argument_parser(description: str) -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=description, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--run-dir", required=True, help="증거를 남길 새 디렉터리")
    parser.add_argument("--app", default=str(DEFAULT_APP), help="Rust 데스크톱 실행 파일(기본 target/release/kanvibe)")
    return parser


def git(repo_path: Path, *arguments: str, environment: dict[str, str]) -> str:
    result = subprocess.run(["git", "-C", str(repo_path), *arguments], env=environment,
                            capture_output=True, text=True, check=True)
    return result.stdout.strip()


def create_repository(repo_path: Path, readme: str, environment: dict[str, str]) -> Path:
    repo_path.mkdir(parents=True, exist_ok=True)
    git(repo_path, "init", "-b", "main", environment=environment)
    git(repo_path, "config", "user.email", "qa@kanvibe.local", environment=environment)
    git(repo_path, "config", "user.name", "KanVibe QA", environment=environment)
    (repo_path / "README.md").write_text(readme, encoding="utf-8")
    git(repo_path, "add", "README.md", environment=environment)
    git(repo_path, "commit", "-m", "chore: isolated mobile QA fixture", environment=environment)
    return repo_path


def managed_worktree_path(repo_path: Path, branch_name: str) -> Path:
    """데스크톱이 브랜치 하나에 만드는 worktree 경로(`<repo>__worktrees/<branch의 /를 ->`)"""
    return repo_path.parent / f"{repo_path.name}__worktrees" / branch_name.replace("/", "-")


def utc_now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%f")[:-3] + "Z"


class ControlClient:
    """QA 제어 소켓. 명령 하나에 응답 한 줄을 받는다. probe 요청이 여러 스레드에서 와도 차례로 보낸다"""

    def __init__(self, socket_path: Path):
        wait_until(socket_path.exists, APP_READY_TIMEOUT_S, f"control socket {socket_path}")
        self.connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.connection.connect(str(socket_path))
        self.reader = self.connection.makefile("r", encoding="utf-8")
        self.lock = threading.Lock()

    def send(self, command: dict, *, check: bool = True) -> dict:
        with self.lock:
            self.connection.sendall((json.dumps(command, ensure_ascii=False) + "\n").encode("utf-8"))
            line = self.reader.readline()
        if not line:
            raise RuntimeError(f"control socket closed during {command.get('cmd')}")
        response = json.loads(line)
        if check and not response.get("ok"):
            raise RuntimeError(f"{command.get('cmd')} failed: {response.get('error')}")
        return response

    def close(self) -> None:
        self.connection.close()


class IsolatedDesktop:
    """Xvfb 화면 하나와 그 위의 Rust 데스크톱 한 벌. `restart()`로 껐다 켜며 그 사이에 DB를 채울 수 있다"""

    def __init__(self, run_dir: Path, app: Path, hook_port: int):
        self.run_dir = run_dir
        self.app = app.resolve()
        if not self.app.exists():
            raise RuntimeError(f"Rust desktop binary not found: {self.app} (cargo build --release -p kanvibe-app)")
        self.hook_port = hook_port
        self.mobile_tls_port = hook_port + MOBILE_TLS_PORT_OFFSET
        self.data_dir = run_dir / "app-data"
        # 이름에 숫자만 넣는다. 모바일 프로젝트 검색은 경로까지 글자 순서로 맞춰 보므로, 임의 글자가 섞이면 엉뚱한 프로젝트가 걸린다
        self.isolation_root = Path(tempfile.gettempdir()) / f"kvm-{os.getpid()}"
        self.isolation_root.mkdir()
        self.home = self.isolation_root / "home"
        self.control_socket_path = self.isolation_root / "control.sock"
        self.display = free_display()
        self.environment = self._build_environment()
        self.xvfb: subprocess.Popen | None = None
        self.process: subprocess.Popen | None = None
        self.control: ControlClient | None = None
        self.launch_count = 0

    def _build_environment(self) -> dict[str, str]:
        """앱과 fixture 자식 프로세스 전용 좁은 환경. 사용자 셸 환경을 통째로 넘기지 않는다"""
        xdg = self.isolation_root / "xdg"
        for directory in (self.home, xdg / "config", xdg / "cache", xdg / "data", xdg / "state",
                          xdg / "runtime", xdg / "tmux"):
            directory.mkdir(parents=True, exist_ok=True)
        (xdg / "runtime").chmod(0o700)
        (xdg / "tmux").chmod(0o700)
        # 격리 HOME에 셸 시작 파일이 없으면 zsh 첫 실행 화면이 fixture가 친 입력을 먹는다
        for startup_file in (".bashrc", ".zshrc", ".profile"):
            (self.home / startup_file).write_text("", encoding="utf-8")
        tool_directories: list[str] = []
        for tool in APP_TOOLS:
            found = shutil.which(tool)
            if found and os.path.dirname(found) not in tool_directories:
                tool_directories.append(os.path.dirname(found))
        return {
            "PATH": ":".join([*tool_directories, "/usr/local/bin", "/usr/bin", "/bin"]),
            "HOME": str(self.home),
            "USER": "kanvibe-qa",
            "LOGNAME": "kanvibe-qa",
            "SHELL": "/bin/bash",
            "TERM": "xterm-256color",
            "LANG": "en_US.UTF-8",
            "TZ": "UTC",
            "DISPLAY": f":{self.display}",
            "XDG_RUNTIME_DIR": str(xdg / "runtime"),
            "XDG_CONFIG_HOME": str(xdg / "config"),
            "XDG_CACHE_HOME": str(xdg / "cache"),
            "XDG_DATA_HOME": str(xdg / "data"),
            "XDG_STATE_HOME": str(xdg / "state"),
            "TMUX_TMPDIR": str(xdg / "tmux"),
            "GIT_CONFIG_NOSYSTEM": "1",
        }

    @property
    def api_url(self) -> str:
        return f"http://127.0.0.1:{self.hook_port}"

    def start(self) -> None:
        if self.xvfb is None:
            self._start_xvfb()
        self.control_socket_path.unlink(missing_ok=True)
        self.launch_count += 1
        log = (self.run_dir / f"desktop-{self.launch_count}.log").open("w")
        command = [
            str(self.app), AUTOMATED_QA_FLAG,
            # 자동 QA 모드는 시각을 이 값에서 실제 경과만큼 흘린다. 지금 시각을 주어 실제 시계와 같게 둔다
            "--fixed-now", datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "--data-dir", str(self.data_dir),
            "--kanvibe-hook-port", str(self.hook_port),
            "--control-socket", str(self.control_socket_path),
            "--window-size", f"{SCREEN_SIZE[0] - 1}x{SCREEN_SIZE[1] - 1}",
        ]
        self.process = subprocess.Popen(command, env=self.environment, stdout=log, stderr=subprocess.STDOUT,
                                        cwd=self.isolation_root, start_new_session=True)
        wait_until(lambda: self._health_status() == 200 or self.process.poll() is not None,
                   APP_READY_TIMEOUT_S, "desktop hook server")
        if self.process.poll() is not None:
            raise RuntimeError(f"desktop exited early ({self.process.returncode}); see {log.name}")
        wait_until(lambda: self._is_listening(self.mobile_tls_port), APP_READY_TIMEOUT_S, "mobile TLS port")
        self.control = ControlClient(self.control_socket_path)
        self.control.send({"cmd": "wait_idle"})
        self.control.send({"cmd": "activate_window"})

    def restart(self) -> None:
        self.quit()
        self.start()

    def quit(self) -> None:
        if self.process is None or self.process.poll() is not None:
            return
        try:
            if self.control is not None:
                self.control.send({"cmd": "quit"}, check=False)
        except (OSError, RuntimeError):
            pass
        try:
            self.process.wait(timeout=APP_QUIT_TIMEOUT_S)
        except subprocess.TimeoutExpired:
            os.killpg(self.process.pid, signal.SIGKILL)
            self.process.wait()
        if self.control is not None:
            self.control.close()
            self.control = None
        wait_until(lambda: not self._is_listening(self.hook_port), APP_QUIT_TIMEOUT_S, "hook port release")

    def stop(self) -> None:
        """앱, 이 실행의 tmux·herdr 서버, 격리 경로로 뜬 남은 프로세스, Xvfb를 끄고 임시 디렉터리를 지운다"""
        try:
            self.quit()
        except RuntimeError:
            pass
        subprocess.run(["tmux", "kill-server"], env=self.environment, capture_output=True)
        if shutil.which("herdr"):
            subprocess.run(["herdr", "server", "stop"], env=self.environment, capture_output=True, timeout=20)
        self._kill_processes_under_isolation_root()
        if self.xvfb is not None and self.xvfb.poll() is None:
            self.xvfb.terminate()
            self.xvfb.wait(timeout=10)
        shutil.rmtree(self.isolation_root, ignore_errors=True)

    def screenshot(self, name: str) -> Path:
        """데스크톱 화면 증거. 실패해도 fixture를 멈추지 않는다"""
        output_path = self.run_dir / f"{name}.png"
        width, height = SCREEN_SIZE
        subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "x11grab", "-draw_mouse", "0",
                        "-video_size", f"{width}x{height}", "-i", f":{self.display}.0+0,0",
                        "-frames:v", "1", str(output_path)], capture_output=True)
        return output_path

    def database(self) -> sqlite3.Connection:
        connection = sqlite3.connect(self.data_dir / "kanvibe.db", timeout=10)
        connection.row_factory = sqlite3.Row
        return connection

    def save_app_setting(self, key: str, value: str) -> None:
        with self.database() as connection:
            connection.execute(
                "INSERT INTO app_settings (id, key, value) VALUES (?, ?, ?) "
                "ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP",
                (str(uuid.uuid4()), key, value))

    def tmux(self, *arguments: str) -> str:
        """이 실행의 tmux 서버(TMUX_TMPDIR)에만 닿는다"""
        result = subprocess.run(["tmux", *arguments], env=self.environment, capture_output=True, text=True,
                                check=True)
        return result.stdout.strip()

    def _start_xvfb(self) -> None:
        width, height = SCREEN_SIZE
        log = (self.run_dir / "xvfb.log").open("w")
        self.xvfb = subprocess.Popen(
            ["Xvfb", f":{self.display}", "-screen", "0", f"{width}x{height}x24", "-nolisten", "tcp", "-dpi", "96"],
            stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        wait_until(lambda: Path(f"/tmp/.X11-unix/X{self.display}").exists(), 10, "Xvfb socket")

    def _health_status(self) -> int | None:
        try:
            connection = http.client.HTTPConnection("127.0.0.1", self.hook_port, timeout=2)
            connection.request("GET", "/api/hooks/health")
            status = connection.getresponse().status
            connection.close()
            return status
        except OSError:
            return None

    @staticmethod
    def _is_listening(port: int) -> bool:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=1):
                return True
        except OSError:
            return False

    def _kill_processes_under_isolation_root(self) -> None:
        result = subprocess.run(["pgrep", "-f", str(self.isolation_root)], capture_output=True, text=True)
        for pid_text in result.stdout.split():
            pid = int(pid_text)
            if pid == os.getpid():
                continue
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass


class MobileObserver:
    """실제 페어링으로 붙인 관측용 기기. 모바일 API가 보는 보드·알림을 fixture와 probe가 같은 경로로 읽는다"""

    def __init__(self, desktop: IsolatedDesktop, device_name: str):
        self.desktop = desktop
        code = self.request("POST", "/api/mobile/dev/pairing-code", body={}, is_authorized=False)["code"]
        paired = self.request("POST", "/api/mobile/pair", body={"code": code, "deviceName": device_name},
                              is_authorized=False)
        self.token = paired["token"]

    def request(self, method: str, path: str, *, body: dict | None = None, is_authorized: bool = True,
                timeout_s: float = 120) -> dict:
        connection = http.client.HTTPConnection("127.0.0.1", self.desktop.hook_port, timeout=timeout_s)
        headers = {"Content-Type": "application/json"}
        if is_authorized:
            headers["Authorization"] = f"Bearer {self.token}"
        payload = None if body is None else json.dumps(body, ensure_ascii=False).encode("utf-8")
        connection.request(method, path, body=payload, headers=headers)
        response = connection.getresponse()
        text = response.read().decode("utf-8")
        connection.close()
        if response.status != 200:
            raise RuntimeError(f"{method} {path} -> HTTP {response.status}: {text}")
        return json.loads(text)

    def board(self) -> dict:
        return self.request("GET", "/api/mobile/board")["board"]

    def notifications(self) -> list[dict]:
        return self.request("GET", "/api/mobile/notifications")["notifications"]


ProbeRoute = Callable[[str, dict[str, str]], dict]


class ProbeServer:
    """Maestro `runScript`가 부르는 작은 HTTP 창구. 경로마다 (method, query) → JSON 응답 함수 하나"""

    def __init__(self, port: int, routes: dict[str, ProbeRoute], observations_path: Path):
        lock = threading.Lock()

        class Handler(BaseHTTPRequestHandler):
            def _answer(self) -> None:
                url = urlparse(self.path)
                route = routes.get(url.path)
                if route is None:
                    self.send_response(404)
                    self.end_headers()
                    return
                query = {key: values[-1] for key, values in parse_qs(url.query).items()}
                try:
                    status, body = 200, route(self.command, query)
                except Exception as error:  # noqa: BLE001 - 실패 이유를 흐름 로그로 돌려준다
                    status, body = 500, {"error": str(error)}
                with lock, observations_path.open("a", encoding="utf-8") as observations:
                    observations.write(json.dumps({"time": utc_now_iso(), "method": self.command, "path": self.path,
                                                   "status": status, "body": body}, ensure_ascii=False) + "\n")
                encoded = json.dumps(body, ensure_ascii=False).encode("utf-8")
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(encoded)))
                self.end_headers()
                self.wfile.write(encoded)

            do_GET = _answer
            do_POST = _answer

            def log_message(self, *_arguments) -> None:
                return

        self.server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def close(self) -> None:
        self.server.shutdown()
        self.server.server_close()


def publish_fixture(run_dir: Path, fixture: dict) -> None:
    (run_dir / "fixture.json").write_text(json.dumps(fixture, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(fixture, ensure_ascii=False), flush=True)


def serve_until_signal(cleanup: Callable[[], None]) -> None:
    """준비를 마친 fixture는 흐름이 끝날 때까지 살아 있다가 SIGINT/SIGTERM에 스스로 정리한다"""
    stopped = threading.Event()
    signal.signal(signal.SIGTERM, lambda *_: stopped.set())
    signal.signal(signal.SIGINT, lambda *_: stopped.set())
    stopped.wait()
    cleanup()


def run_fixture(prepare: Callable[[IsolatedDesktop], tuple[dict, Callable[[], None]]],
                run_dir: Path, app: Path, hook_port: int, fixed_ports: list[int]) -> None:
    """포트를 확인하고 데스크톱을 띄워 `prepare`에 맡긴 뒤, 신호를 받을 때까지 기다렸다가 정리한다"""
    for port in [hook_port, hook_port + MOBILE_TLS_PORT_OFFSET, *fixed_ports]:
        assert_port_free(port)
    desktop = IsolatedDesktop(run_dir, app, hook_port)
    try:
        fixture, close_probe = prepare(desktop)
    except BaseException:
        desktop.screenshot("failure-desktop")
        desktop.stop()
        raise

    def cleanup() -> None:
        close_probe()
        desktop.screenshot("final-desktop")
        desktop.stop()

    publish_fixture(run_dir, fixture)
    serve_until_signal(cleanup)
