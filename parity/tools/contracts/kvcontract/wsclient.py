"""표준 라이브러리만으로 만든 최소 RFC 6455 WebSocket 클라이언트.

녹화기와 재생기가 외부 패키지 설치 없이 돌아야 해서 직접 구현한다. 서버 응답을 있는 그대로 관찰하는 것이 목적이라
핸드셰이크가 101이 아니면 예외 대신 원문 응답을 돌려주고, 연결이 끊긴 방식도 구분해서 보고한다.
"""

from __future__ import annotations

import base64
import hashlib
import os
import socket
import ssl
import struct
import time
from dataclasses import dataclass, field
from urllib.parse import urlsplit

OPCODE_CONTINUATION = 0x0
OPCODE_TEXT = 0x1
OPCODE_BINARY = 0x2
OPCODE_CLOSE = 0x8
OPCODE_PING = 0x9
OPCODE_PONG = 0xA


@dataclass
class HandshakeResult:
    """업그레이드 요청에 서버가 보인 반응. 101이 아니면 받은 원문을 그대로 남긴다"""

    status: int | None
    raw_response: bytes
    headers: dict[str, str] = field(default_factory=dict)
    peer_certificate_sha256: str | None = None


@dataclass
class ReceivedFrame:
    kind: str  # text | binary | close | timeout | eof
    text: str | None = None
    data: bytes | None = None
    close_code: int | None = None
    close_reason: str | None = None


class WebSocketConnection:
    def __init__(self, sock: socket.socket, leftover: bytes):
        self._sock = sock
        self._buffer = leftover
        self.is_closed = False

    def _read_exact(self, size: int, deadline: float) -> bytes | None:
        while len(self._buffer) < size:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError
            self._sock.settimeout(remaining)
            try:
                chunk = self._sock.recv(65536)
            except socket.timeout as error:
                raise TimeoutError from error
            except (ConnectionResetError, ssl.SSLError, OSError):
                return None
            if not chunk:
                return None
            self._buffer += chunk
        data, self._buffer = self._buffer[:size], self._buffer[size:]
        return data

    def _read_frame(self, deadline: float):
        header = self._read_exact(2, deadline)
        if header is None:
            return None
        first, second = header
        is_final = bool(first & 0x80)
        opcode = first & 0x0F
        length = second & 0x7F
        if length == 126:
            extended = self._read_exact(2, deadline)
            if extended is None:
                return None
            length = struct.unpack("!H", extended)[0]
        elif length == 127:
            extended = self._read_exact(8, deadline)
            if extended is None:
                return None
            length = struct.unpack("!Q", extended)[0]
        mask = None
        if second & 0x80:
            mask = self._read_exact(4, deadline)
        payload = self._read_exact(length, deadline) if length else b""
        if payload is None:
            return None
        if mask:
            payload = bytes(byte ^ mask[index % 4] for index, byte in enumerate(payload))
        return is_final, opcode, payload

    def _send_frame(self, opcode: int, payload: bytes) -> None:
        header = bytearray([0x80 | opcode])
        length = len(payload)
        if length < 126:
            header.append(0x80 | length)
        elif length < 65536:
            header.append(0x80 | 126)
            header += struct.pack("!H", length)
        else:
            header.append(0x80 | 127)
            header += struct.pack("!Q", length)
        mask = os.urandom(4)
        header += mask
        masked = bytes(byte ^ mask[index % 4] for index, byte in enumerate(payload))
        self._sock.sendall(bytes(header) + masked)

    def send_text(self, text: str) -> None:
        self._send_frame(OPCODE_TEXT, text.encode("utf-8"))

    def send_binary(self, data: bytes) -> None:
        self._send_frame(OPCODE_BINARY, data)

    def receive(self, timeout: float) -> ReceivedFrame:
        """데이터 프레임 하나나 close를 기다린다. ping에는 알아서 pong으로 답한다"""
        deadline = time.monotonic() + timeout
        fragments: list[bytes] = []
        fragment_opcode = None
        while True:
            try:
                frame = self._read_frame(deadline)
            except TimeoutError:
                return ReceivedFrame(kind="timeout")
            if frame is None:
                self.is_closed = True
                return ReceivedFrame(kind="eof")
            is_final, opcode, payload = frame
            if opcode == OPCODE_PING:
                self._send_frame(OPCODE_PONG, payload)
                continue
            if opcode == OPCODE_PONG:
                continue
            if opcode == OPCODE_CLOSE:
                self.is_closed = True
                code = struct.unpack("!H", payload[:2])[0] if len(payload) >= 2 else None
                reason = payload[2:].decode("utf-8", "replace") if len(payload) > 2 else ""
                try:
                    self._send_frame(OPCODE_CLOSE, payload[:2])
                except OSError:
                    pass
                return ReceivedFrame(kind="close", close_code=code, close_reason=reason)
            if opcode in (OPCODE_TEXT, OPCODE_BINARY):
                fragment_opcode = opcode
                fragments = [payload]
            elif opcode == OPCODE_CONTINUATION:
                fragments.append(payload)
            if not is_final:
                continue
            data = b"".join(fragments)
            if fragment_opcode == OPCODE_TEXT:
                return ReceivedFrame(kind="text", text=data.decode("utf-8", "replace"))
            return ReceivedFrame(kind="binary", data=data)

    def close(self, code: int = 1000, reason: str = "") -> None:
        if not self.is_closed:
            try:
                self._send_frame(OPCODE_CLOSE, struct.pack("!H", code) + reason.encode("utf-8"))
            except OSError:
                pass
        self.is_closed = True
        try:
            self._sock.close()
        except OSError:
            pass


def _open_socket(url: str, timeout: float) -> tuple[socket.socket, str, str, str | None]:
    parts = urlsplit(url)
    is_secure = parts.scheme in ("wss", "https")
    port = parts.port or (443 if is_secure else 80)
    raw = socket.create_connection((parts.hostname, port), timeout=timeout)
    fingerprint = None
    if is_secure:
        context = ssl.create_default_context()
        context.check_hostname = False
        context.verify_mode = ssl.CERT_NONE
        raw = context.wrap_socket(raw, server_hostname=parts.hostname)
        der = raw.getpeercert(binary_form=True)
        fingerprint = hashlib.sha256(der).hexdigest() if der else None
    target = parts.path or "/"
    if parts.query:
        target += "?" + parts.query
    return raw, f"{parts.hostname}:{port}", target, fingerprint


def connect(url: str, headers: dict[str, str] | None = None, timeout: float = 10.0):
    """업그레이드를 시도한다. 성공하면 (HandshakeResult, WebSocketConnection), 아니면 (HandshakeResult, None)"""
    sock, host_header, target, fingerprint = _open_socket(url, timeout)
    key = base64.b64encode(os.urandom(16)).decode("ascii")
    lines = [
        f"GET {target} HTTP/1.1",
        f"Host: {host_header}",
        "Upgrade: websocket",
        "Connection: Upgrade",
        f"Sec-WebSocket-Key: {key}",
        "Sec-WebSocket-Version: 13",
    ]
    for name, value in (headers or {}).items():
        lines.append(f"{name}: {value}")
    sock.sendall(("\r\n".join(lines) + "\r\n\r\n").encode("utf-8"))

    received = b""
    deadline = time.monotonic() + timeout
    while b"\r\n\r\n" not in received:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            break
        sock.settimeout(remaining)
        try:
            chunk = sock.recv(4096)
        except (socket.timeout, ConnectionResetError, ssl.SSLError, OSError):
            break
        if not chunk:
            break
        received += chunk

    head, _, leftover = received.partition(b"\r\n\r\n")
    status = None
    response_headers: dict[str, str] = {}
    if head:
        status_line, *header_lines = head.decode("latin-1").split("\r\n")
        pieces = status_line.split(" ", 2)
        if len(pieces) >= 2 and pieces[1].isdigit():
            status = int(pieces[1])
        for line in header_lines:
            name, _, value = line.partition(":")
            response_headers[name.strip().lower()] = value.strip()
    result = HandshakeResult(status=status, raw_response=received if status != 101 else head + b"\r\n\r\n",
                             headers=response_headers, peer_certificate_sha256=fingerprint)
    if status != 101:
        # 서버가 원문을 쓰고 끊는 경우가 있어, 남은 바이트를 마저 읽어 원문 전체를 남긴다
        try:
            sock.settimeout(0.5)
            while True:
                chunk = sock.recv(4096)
                if not chunk:
                    break
                result.raw_response += chunk
        except (socket.timeout, ConnectionResetError, ssl.SSLError, OSError):
            pass
        sock.close()
        return result, None
    return result, WebSocketConnection(sock, leftover)
