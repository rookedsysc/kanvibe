"""Rust 서버의 QA 실행기(`kanvibe-server-qa`)를 골든을 녹화할 때와 같은 격리 환경에서 띄운다.

`child_environment.build_child_environment`로 자식 env를 만들므로 PATH·HOME·tmux 소켓이 골든을 녹화할 때와 같다.
오라클(`issue_pairing_code`, `issue_hook_install_ticket`)은 같은 실행 파일의 `oracle` 하위 명령으로 실행기의 Unix 소켓에 묻는다.
"""

from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import tempfile
import time
from pathlib import Path

from .child_environment import DEFAULT_CHILD_PATH, MOBILE_TLS_PORT_OFFSET, build_child_environment, is_port_free


class RustApp:
    """실행 중인 격리 Rust 서버 한 개"""

    def __init__(self, binary: Path, hook_port: int, run_dir: Path | None = None):
        self.binary = binary.resolve()
        self.hook_port = hook_port
        self.tls_port = hook_port + MOBILE_TLS_PORT_OFFSET
        self.owns_run_dir = run_dir is None
        self.run_dir = (run_dir or Path(tempfile.mkdtemp(prefix="kanvibe-contract-rust-"))).resolve()
        self.socket_dir = Path(tempfile.mkdtemp(prefix="kvr-"))
        self.oracle_socket = self.socket_dir / "oracle.sock"
        self.log_path = self.run_dir / "server.log"
        self.process: subprocess.Popen | None = None

    @property
    def plain_url(self) -> str:
        return f"http://127.0.0.1:{self.hook_port}"

    @property
    def tls_url(self) -> str:
        return f"https://127.0.0.1:{self.tls_port}"

    @property
    def oracle_command(self) -> str:
        return f"{self.binary} oracle --socket {self.oracle_socket}"

    def start(self, timeout: float = 60.0) -> None:
        for port in (self.hook_port, self.tls_port):
            if not is_port_free(port):
                raise RuntimeError(f"port {port} is already in use; pass another --hook-port")
        environment = build_child_environment(self.run_dir, self.socket_dir)
        command = [
            str(self.binary), "serve",
            "--data-dir", environment["KANVIBE_APP_DATA_DIR"],
            "--hook-port", str(self.hook_port),
            "--oracle-socket", str(self.oracle_socket),
        ]
        log_file = open(self.log_path, "wb")
        self.process = subprocess.Popen(
            command, cwd=self.run_dir, env=environment, stdout=log_file, stderr=subprocess.STDOUT,
            stdin=subprocess.DEVNULL, start_new_session=True,
        )
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError(f"Rust server exited early ({self.process.returncode}); see {self.log_path}")
            if self.oracle_socket.exists() and not is_port_free(self.hook_port) and not is_port_free(self.tls_port):
                return
            time.sleep(0.2)
        raise RuntimeError(f"Rust server did not become ready in {timeout}s; see {self.log_path}")

    def run_oracle(self, name: str, args: dict | None = None):
        completed = subprocess.run(
            [str(self.binary), "oracle", "--socket", str(self.oracle_socket), name, json.dumps(args or {})],
            capture_output=True, text=True, check=True, env={"PATH": DEFAULT_CHILD_PATH},
        )
        return json.loads(completed.stdout)

    def stop(self) -> None:
        if self.process and self.process.poll() is None:
            try:
                os.killpg(self.process.pid, signal.SIGTERM)
                self.process.wait(timeout=15)
            except (ProcessLookupError, subprocess.TimeoutExpired):
                try:
                    os.killpg(self.process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
        self._kill_isolated_tmux()
        if self.owns_run_dir:
            shutil.rmtree(self.run_dir, ignore_errors=True)

    def _kill_isolated_tmux(self) -> None:
        """격리 소켓에 남은 tmux 서버만 끈다. 사용자 서버에는 닿지 않는다"""
        tmux_dir = self.socket_dir / "tmux"
        if tmux_dir.exists() and shutil.which("tmux"):
            environment = {"PATH": DEFAULT_CHILD_PATH, "TMUX_TMPDIR": str(tmux_dir), "HOME": str(self.run_dir / "home")}
            subprocess.run(["tmux", "kill-server"], env=environment, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
        shutil.rmtree(self.socket_dir, ignore_errors=True)
