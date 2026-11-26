"""앱의 QA 제어 소켓(`kanvibe --qa-harness`)에 한 줄 JSON 명령을 보내고 한 줄 응답을 받는 클라이언트."""
from __future__ import annotations

import json
import socket
import time
from pathlib import Path


class ControlClient:
    """QA 제어 소켓 클라이언트. 명령 하나에 응답 한 줄을 기다린다."""

    def __init__(self, socket_path: Path, timeout_s: float = 30):
        deadline = time.monotonic() + timeout_s
        while not socket_path.exists():
            if time.monotonic() > deadline:
                raise RuntimeError(f"control socket did not appear: {socket_path}")
            time.sleep(0.05)
        self.connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.connection.connect(str(socket_path))
        self.reader = self.connection.makefile("r", encoding="utf-8")

    def send(self, command: dict) -> dict:
        self.connection.sendall((json.dumps(command, ensure_ascii=False) + "\n").encode("utf-8"))
        response = json.loads(self.reader.readline())
        if not response.get("ok"):
            raise RuntimeError(f"{command.get('cmd')} failed: {response.get('error')}")
        return response
