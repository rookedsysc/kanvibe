#!/usr/bin/env python3
"""모바일의 PC 저장·전환 여정(multi-pc, chat-history-multi-pc)만 검증하는 두 개의 격리된 가짜 PC API.

19836("Studio task")과 19837("Home task")에서 자체 서명 인증서로 HTTPS를 연다. 페어링 코드는 123456이다.
실제 KanVibe API 계약은 `prepare_qa_desktop.py` 여정이 Rust 데스크톱으로 검증한다. 인증서는 `openssl`로 만들고
`--run-dir/tls/<포트>`에 둔다. SIGINT/SIGTERM을 받으면 끝난다.

예:
  python3 qa/mobile/multi_pc_fixture.py --run-dir qa-results/mobile/multi-pc-1
"""
from __future__ import annotations

import argparse
import json
import ssl
import subprocess
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from desktop_fixture import require_fresh_run_dir, serve_until_signal  # noqa: E402

PCS = ((19836, "Studio task"), (19837, "Home task"))
PAIRING_CODE = "123456"
CERTIFICATE_DAYS = 3650


def create_certificate(directory: Path) -> tuple[Path, Path]:
    directory.mkdir(parents=True, exist_ok=True)
    certificate_path, key_path = directory / "cert.pem", directory / "key.pem"
    subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-sha256", "-nodes",
                    "-days", str(CERTIFICATE_DAYS), "-subj", "/CN=KanVibe QA PC",
                    "-keyout", str(key_path), "-out", str(certificate_path)], check=True, capture_output=True)
    return certificate_path, key_path


def board_for(port: int, title: str) -> dict:
    return {
        "projects": [
            {"id": "primary", "name": "Primary", "repoPath": "/qa/primary"},
            {"id": "secondary", "name": "Secondary", "repoPath": "/qa/secondary"},
        ],
        "tasks": {
            "todo": [
                {"id": f"qa-{port}", "title": title, "status": "todo", "projectId": "primary"},
                {"id": f"qa-secondary-{port}", "title": f"{title} secondary", "status": "todo",
                 "projectId": "secondary"},
            ],
            "progress": [], "pending": [], "review": [], "done": [],
        },
    }


def start_pc(port: int, title: str, certificate: tuple[Path, Path]) -> ThreadingHTTPServer:
    token = f"qa-token-{port}"
    state = {"is_paired": False}

    class Handler(BaseHTTPRequestHandler):
        def _json(self, status: int, body: dict) -> None:
            encoded = json.dumps(body).encode("utf-8")
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(encoded)))
            self.end_headers()
            self.wfile.write(encoded)

        def _answer(self) -> None:
            if self.path == "/health":
                return self._json(200, {"ready": True})
            if self.path == "/api/mobile/pair" and self.command == "POST":
                length = int(self.headers.get("Content-Length") or 0)
                body = json.loads(self.rfile.read(length) or b"{}")
                if body.get("code") != PAIRING_CODE:
                    return self._json(401, {"error": "wrong code"})
                state["is_paired"] = True
                return self._json(200, {"token": token})
            if not state["is_paired"] or self.headers.get("Authorization") != f"Bearer {token}":
                return self._json(401, {"error": "unauthorized"})
            if self.path == "/api/mobile/board" and self.command == "GET":
                return self._json(200, {"board": board_for(port, title)})
            if self.path == "/api/mobile/device" and self.command == "DELETE":
                state["is_paired"] = False
                return self._json(200, {"success": True})
            return self._json(404, {"error": "not found"})

        do_GET = _answer
        do_POST = _answer
        do_DELETE = _answer

        def log_message(self, *_arguments) -> None:
            return

    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(*map(str, certificate))
    server.socket = context.wrap_socket(server.socket, server_side=True)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    print(f"QA PC {port} ready", flush=True)
    return server


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--run-dir", required=True, help="인증서를 둘 새 디렉터리")
    run_dir = require_fresh_run_dir(parser.parse_args().run_dir)
    servers = [start_pc(port, title, create_certificate(run_dir / "tls" / str(port))) for port, title in PCS]

    def close() -> None:
        for server in servers:
            server.shutdown()
            server.server_close()

    serve_until_signal(close)


if __name__ == "__main__":
    main()
