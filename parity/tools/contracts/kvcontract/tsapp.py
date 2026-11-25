"""빌드된 Electron(TS) 앱을 격리된 디렉터리에서 띄우고, 메인 프로세스 inspector로 오라클 값을 받아 온다.

격리 원칙(CLAUDE.md 환경 안전 규칙):
- 자식 프로세스 env는 부모 env를 통째로 넘기지 않고 필요한 값만 골라 새로 만든다. 부모의 TMUX/ZELLIJ/KANVIBE_* 값은 넘기지 않는다.
- HOME, XDG_*, TMUX_TMPDIR를 실행 디렉터리 안으로 돌려 사용자의 tmux 서버, 셸 설정, CLI 계정 파일에 닿지 않게 한다.
- 이 값들은 띄운 자식에게만 적용하고 녹화기 자신의 os.environ은 바꾸지 않는다.
"""

from __future__ import annotations

import http.client
import json
import os
import shutil
import signal
import socket
import ssl
import subprocess
import tempfile
import time
from pathlib import Path

from . import wsclient

REPO_ROOT = Path(__file__).resolve().parents[4]
ELECTRON_BINARY = REPO_ROOT / "node_modules" / "electron" / "dist" / "electron"
BUILD_MAIN_SRC = REPO_ROOT / "build" / "main" / "src"
MOBILE_TLS_PORT_OFFSET = 10

# 오라클 `issue_hook_install_ticket`에 넘기는 고정 입력. 같은 입력이면 설치 스크립트 본문이 결정적이다
HOOK_INSTALL_ORACLE_FILES = [
    {"filePath": "/tmp/kanvibe-contract-hook/status.sh", "content": "echo kanvibe-contract\n", "mode": 0o755},
]

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
        # Node도 SO_REUSEADDR로 listen하므로, 직전 실행의 TIME_WAIT 연결은 포트 사용 중으로 보지 않는다
        probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            probe.bind(("127.0.0.1", port))
        except OSError:
            return False
    return True


class InspectorOracle:
    """TS 앱 메인 프로세스의 Node inspector로 시험용 값을 얻는다. 재생기가 이미 떠 있는 TS 앱에 붙을 때도 쓴다"""

    def __init__(self, inspector_port: int):
        self.inspector_port = inspector_port

    def _inspector_target(self) -> str | None:
        try:
            connection = http.client.HTTPConnection("127.0.0.1", self.inspector_port, timeout=2)
            connection.request("GET", "/json/list")
            targets = json.loads(connection.getresponse().read())
        except (OSError, ValueError):
            return None
        for target in targets:
            if target.get("webSocketDebuggerUrl"):
                return target["webSocketDebuggerUrl"]
        return None

    def evaluate_in_main(self, expression: str):
        """메인 프로세스에서 식을 평가해 JSON 값으로 돌려받는다. 테스트 오라클 전용"""
        url = self._inspector_target()
        if not url:
            raise RuntimeError("inspector target not available")
        _, connection = wsclient.connect(url, timeout=10)
        if connection is None:
            raise RuntimeError("inspector websocket refused")
        try:
            connection.send_text(json.dumps({
                "id": 1,
                "method": "Runtime.evaluate",
                "params": {"expression": expression, "awaitPromise": True, "returnByValue": True},
            }))
            while True:
                frame = connection.receive(timeout=20)
                if frame.kind != "text":
                    raise RuntimeError(f"inspector closed: {frame}")
                message = json.loads(frame.text)
                if message.get("id") != 1:
                    continue
                result = message.get("result", {})
                if "exceptionDetails" in result:
                    raise RuntimeError(f"inspector evaluation failed: {result['exceptionDetails']}")
                return result.get("result", {}).get("value")
        finally:
            connection.close()

    def _require_expression(self, module_relative: str) -> str:
        module_path = json.dumps(str(BUILD_MAIN_SRC / module_relative))
        return f"process.getBuiltinModule('node:module').createRequire({module_path})({module_path})"

    def run_oracle(self, name: str, args: dict | None = None):
        if name == "issue_pairing_code":
            service = self._require_expression("desktop/main/services/mobileBridgeService.js")
            return self.evaluate_in_main(f"{service}.startMobilePairing()")
        if name == "issue_hook_install_ticket":
            bundle = self._require_expression("lib/hookInstallBundle.js")
            files = json.dumps(HOOK_INSTALL_ORACLE_FILES)
            return self.evaluate_in_main(f"{bundle}.issueHookInstallScript({files})")
        raise RuntimeError(f"unknown oracle {name}")


class TsApp(InspectorOracle):
    """실행 중인 격리 TS 앱 한 개"""

    def __init__(self, hook_port: int, inspector_port: int, run_dir: Path | None = None, keep_run_dir: bool = False):
        super().__init__(inspector_port)
        self.hook_port = hook_port
        self.tls_port = hook_port + MOBILE_TLS_PORT_OFFSET
        self.inspector_port = inspector_port
        self.owns_run_dir = run_dir is None
        self.run_dir = (run_dir or Path(tempfile.mkdtemp(prefix="kanvibe-contract-"))).resolve()
        self.keep_run_dir = keep_run_dir
        self.process: subprocess.Popen | None = None
        self.log_path = self.run_dir / "electron.log"
        self.socket_dir = Path(tempfile.mkdtemp(prefix="kvc-"))
        self.environment: dict[str, str] = {}

    @property
    def app_data_dir(self) -> Path:
        return self.run_dir / "appdata"

    @property
    def plain_url(self) -> str:
        return f"http://127.0.0.1:{self.hook_port}"

    @property
    def tls_url(self) -> str:
        return f"https://127.0.0.1:{self.tls_port}"

    def start(self, timeout: float = 90.0) -> None:
        # 직전 실행의 Electron이 아직 내려가는 중일 수 있어 잠깐 기다린 뒤에만 포트 충돌로 본다
        port_deadline = time.monotonic() + 20
        for port in (self.hook_port, self.tls_port, self.inspector_port):
            while not is_port_free(port):
                if time.monotonic() > port_deadline:
                    raise RuntimeError(f"port {port} is already in use; pass another --hook-port/--inspector-port")
                time.sleep(0.5)
        if not (BUILD_MAIN_SRC / "desktop" / "main" / "services" / "mobileBridgeService.js").exists():
            raise RuntimeError("build/main is missing; the TS app must be built (pnpm build) before recording")
        self.environment = build_child_environment(self.run_dir, self.socket_dir)
        command = [
            "xvfb-run", "-a", "-s", "-screen 0 1600x1000x24",
            str(ELECTRON_BINARY),
            f"--inspect={self.inspector_port}",
            f"--kanvibe-hook-port={self.hook_port}",
            "--no-sandbox",
            str(REPO_ROOT),
        ]
        log_file = open(self.log_path, "wb")
        self.process = subprocess.Popen(
            command, cwd=REPO_ROOT, env=self.environment, stdout=log_file, stderr=subprocess.STDOUT,
            stdin=subprocess.DEVNULL, start_new_session=True,
        )
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError(f"Electron exited early ({self.process.returncode}); see {self.log_path}")
            if self._is_plain_ready() and self._is_tls_ready() and self._inspector_target():
                return
            time.sleep(0.5)
        raise RuntimeError(f"Electron did not become ready in {timeout}s; see {self.log_path}")

    def _is_plain_ready(self) -> bool:
        try:
            connection = http.client.HTTPConnection("127.0.0.1", self.hook_port, timeout=2)
            connection.request("GET", "/api/hooks/health")
            return connection.getresponse().status == 200
        except OSError:
            return False

    def _is_tls_ready(self) -> bool:
        context = ssl.create_default_context()
        context.check_hostname = False
        context.verify_mode = ssl.CERT_NONE
        try:
            with socket.create_connection(("127.0.0.1", self.tls_port), timeout=2) as raw:
                with context.wrap_socket(raw, server_hostname="127.0.0.1"):
                    return True
        except OSError:
            return False

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
        if self.owns_run_dir and not self.keep_run_dir:
            shutil.rmtree(self.run_dir, ignore_errors=True)

    def _kill_isolated_tmux(self) -> None:
        """격리 소켓에 남은 tmux 서버만 끈다. TMUX_TMPDIR를 실행 디렉터리로 주므로 사용자 서버에는 닿지 않는다"""
        tmux_dir = self.socket_dir / "tmux"
        if tmux_dir.exists() and shutil.which("tmux"):
            environment = {"PATH": DEFAULT_CHILD_PATH, "TMUX_TMPDIR": str(tmux_dir), "HOME": str(self.run_dir / "home")}
            subprocess.run(["tmux", "kill-server"], env=environment, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
        shutil.rmtree(self.socket_dir, ignore_errors=True)
