"""골든 케이스를 순서대로 실행한다. 녹화(record)와 재생(replay)이 같은 실행 경로를 쓴다.

케이스는 앞 케이스가 만든 상태(페어링 토큰, 프로젝트, 태스크)에 기대므로 seq 순서대로 실행하고,
`capture`로 응답에서 뽑은 값을 변수에 넣어 뒤 케이스의 `{{var:x}}` 자리에 채운다.
"""

from __future__ import annotations

import base64
import hashlib
import http.client
import json
import socket
import ssl
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable
from urllib.parse import urlsplit

from . import wsclient
from .fixture import run_fixture_git
from .matching import Normalizer, extract, interpolate, match

RECORDED_HEADERS = ("content-type", "cache-control")
DEFAULT_HTTP_TIMEOUT = 60.0


@dataclass
class Endpoints:
    plain_url: str
    tls_url: str

    def base_for(self, listener: str) -> str:
        return self.tls_url if listener == "tls" else self.plain_url


@dataclass
class CaseResult:
    case_id: str
    is_passed: bool
    errors: list[str] = field(default_factory=list)
    observed: Any = None


def _tls_context() -> ssl.SSLContext:
    context = ssl.create_default_context()
    context.check_hostname = False
    context.verify_mode = ssl.CERT_NONE
    return context


def _build_body(request: dict) -> bytes | None:
    if "body_fill" in request:
        fill = request["body_fill"]
        prefix = fill.get("prefix", "").encode("utf-8")
        suffix = fill.get("suffix", "").encode("utf-8")
        filler = fill.get("char", "a").encode("utf-8") * (fill["bytes"] - len(prefix) - len(suffix))
        return prefix + filler + suffix
    if "body_text" in request:
        return request["body_text"].encode("utf-8")
    if "body" in request and request["body"] is not None:
        return json.dumps(request["body"], ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    return None


def _decode_body(raw: bytes, content_type: str | None) -> dict:
    text = raw.decode("utf-8", "replace")
    if content_type and content_type.startswith("application/json"):
        try:
            return {"body": json.loads(text)}
        except ValueError:
            return {"body_text": text}
    return {"body_text": text}


def execute_http(endpoints: Endpoints, request: dict) -> dict:
    """요청 하나를 새 연결로 보낸다. 응답을 받지 못하고 연결이 끊기면 outcome=no-response"""
    base = urlsplit(endpoints.base_for(request.get("listener", "plain")))
    timeout = request.get("timeout", DEFAULT_HTTP_TIMEOUT)
    if base.scheme == "https":
        connection = http.client.HTTPSConnection(base.hostname, base.port, timeout=timeout, context=_tls_context())
    else:
        connection = http.client.HTTPConnection(base.hostname, base.port, timeout=timeout)
    body = _build_body(request)
    try:
        connection.putrequest(request["method"], request["path"], skip_accept_encoding=True)
        for name, value in request.get("headers", {}).items():
            connection.putheader(name, value)
        if body is not None:
            connection.putheader("Content-Length", str(len(body)))
        connection.endheaders()
        fingerprint = None
        if base.scheme == "https" and connection.sock is not None:
            der = connection.sock.getpeercert(binary_form=True)
            fingerprint = hashlib.sha256(der).hexdigest() if der else None
        if body is not None:
            connection.send(body)
        response = connection.getresponse()
        raw = response.read()
        content_type = response.getheader("content-type")
        observed = {
            "status": response.status,
            "headers": {name: response.getheader(name) for name in RECORDED_HEADERS},
            **_decode_body(raw, content_type),
        }
        if fingerprint:
            observed["tls"] = {"peer_certificate_sha256": fingerprint}
        return observed
    except (ConnectionResetError, BrokenPipeError, http.client.RemoteDisconnected, http.client.BadStatusLine, ssl.SSLError) as error:
        return {"outcome": "no-response", "detail": type(error).__name__}
    finally:
        connection.close()


def _frame_to_observation(frame: wsclient.ReceivedFrame) -> dict:
    if frame.kind == "text":
        # 터미널 출력 조각이 "42"처럼 우연히 JSON 스칼라일 수 있어, 객체·배열일 때만 JSON으로 본다
        try:
            parsed = json.loads(frame.text)
        except ValueError:
            parsed = None
        if isinstance(parsed, (dict, list)):
            return {"kind": "text", "json": parsed}
        return {"kind": "text", "text": frame.text}
    if frame.kind == "binary":
        return {"kind": "binary", "base64": base64.b64encode(frame.data).decode("ascii")}
    if frame.kind == "close":
        return {"kind": "close", "code": frame.close_code, "reason": frame.close_reason}
    return {"kind": frame.kind}


def _frame_matches(observation: dict, step: dict) -> bool:
    if "where" in step:
        payload = observation.get("json")
        return isinstance(payload, dict) and all(payload.get(key) == value for key, value in step["where"].items())
    if "contains" in step:
        return observation.get("kind") == "text" and step["contains"] in observation.get("text", "")
    return True


def execute_ws(endpoints: Endpoints, request: dict, steps: list[dict]) -> dict:
    base = endpoints.base_for(request.get("listener", "plain"))
    scheme = "wss" if base.startswith("https") else "ws"
    url = f"{scheme}://{urlsplit(base).netloc}{request['path']}"
    try:
        handshake, connection = wsclient.connect(url, request.get("headers", {}), timeout=request.get("timeout", 15))
    except (ConnectionResetError, ssl.SSLError, OSError) as error:
        return {"handshake": {"outcome": "no-response", "detail": type(error).__name__}, "frames": []}
    handshake_observation: dict[str, Any]
    if handshake.status is None:
        handshake_observation = {"outcome": "no-response"}
    elif handshake.status == 101:
        handshake_observation = {"status": 101}
    else:
        handshake_observation = {"status": handshake.status, "raw": handshake.raw_response.decode("latin-1")}
    if handshake.peer_certificate_sha256:
        handshake_observation["tls"] = {"peer_certificate_sha256": handshake.peer_certificate_sha256}
    observed = {"handshake": handshake_observation, "frames": []}
    if connection is None:
        return observed
    try:
        for step in steps:
            operation = step["op"]
            if operation == "send_text":
                connection.send_text(json.dumps(step["json"], separators=(",", ":")) if "json" in step else step["text"])
            elif operation == "send_binary":
                payload = json.dumps(step["json"], separators=(",", ":")).encode("utf-8") if "json" in step else base64.b64decode(step["base64"])
                connection.send_binary(payload)
            elif operation == "sleep":
                time.sleep(step["seconds"])
            elif operation == "http":
                result = execute_http(endpoints, step["request"])
                observed["frames"].append({"kind": "http", "status": result.get("status"), "body": result.get("body")})
            elif operation == "close":
                connection.close(step.get("code", 1000))
            elif operation in ("recv", "recv_until", "expect_close"):
                deadline = time.monotonic() + step.get("timeout", 10)
                # 터미널 출력은 조각 경계가 실행마다 달라서, contains 대기는 받은 텍스트를 이어 붙여 찾는다
                accumulated_text = ""
                while True:
                    remaining = max(0.05, deadline - time.monotonic())
                    frame = connection.receive(remaining)
                    observation = _frame_to_observation(frame)
                    is_terminal = observation["kind"] in ("close", "eof", "timeout")
                    if operation == "recv":
                        break
                    if operation == "expect_close" and is_terminal:
                        break
                    if operation == "recv_until" and "contains" in step and frame.kind == "text":
                        accumulated_text += frame.text
                        if step["contains"] in accumulated_text:
                            observation = {"kind": "text", "text": accumulated_text}
                            break
                        continue
                    if operation == "recv_until" and (is_terminal or _frame_matches(observation, step)):
                        break
                observed["frames"].append(observation)
            else:
                raise ValueError(f"unknown ws step {operation}")
    finally:
        connection.close()
    return observed


def execute_tls_certificate(endpoints: Endpoints) -> dict:
    """TLS 포트가 내보이는 인증서의 지문과 모양. 지문은 앱이 TOFU로 고정하는 값이라 실행 내내 같아야 한다"""
    base = urlsplit(endpoints.tls_url)
    with socket.create_connection((base.hostname, base.port), timeout=10) as raw:
        with _tls_context().wrap_socket(raw, server_hostname=base.hostname) as secure:
            der = secure.getpeercert(binary_form=True)
            tls_version = secure.version()
    observed: dict[str, Any] = {"peer_certificate_sha256": hashlib.sha256(der).hexdigest(), "tls_version_at_least_1_2": tls_version in ("TLSv1.2", "TLSv1.3")}
    try:
        from cryptography import x509
        from cryptography.hazmat.primitives.asymmetric import rsa
        from cryptography.x509.oid import NameOID
    except ImportError:
        observed["certificate"] = "unparsed (python cryptography package missing)"
        return observed
    certificate = x509.load_der_x509_certificate(der)
    public_key = certificate.public_key()
    not_before = certificate.not_valid_before_utc
    not_after = certificate.not_valid_after_utc
    observed["certificate"] = {
        "subject_common_name": certificate.subject.get_attributes_for_oid(NameOID.COMMON_NAME)[0].value,
        "is_self_issued": certificate.subject == certificate.issuer,
        "key_type": "rsa" if isinstance(public_key, rsa.RSAPublicKey) else type(public_key).__name__,
        "key_bits": public_key.key_size,
        "signature_hash": certificate.signature_hash_algorithm.name if certificate.signature_hash_algorithm else None,
        "validity_days": round((not_after - not_before).total_seconds() / 86400),
    }
    return observed


class OracleBackend:
    """서버마다 다른 '시험용 값 얻기'를 감싼다. TS 앱은 inspector, 다른 서버는 명령이나 dev 경로"""

    def __init__(self, server_oracle: Callable[[str, dict], Any] | None):
        self.server_oracle = server_oracle

    def run(self, oracle: dict, variables: dict) -> Any:
        kind = oracle["name"]
        if kind == "write_file":
            path = Path(interpolate(oracle["args"]["path"], variables))
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(oracle["args"]["content"])
            return None
        if kind == "git":
            arguments = interpolate(oracle["args"]["args"], variables)
            run_fixture_git(Path(interpolate(oracle["args"]["cwd"], variables)), *arguments)
            return None
        if kind == "sleep":
            time.sleep(oracle["args"]["seconds"])
            return None
        if kind == "read_json_file":
            path = Path(interpolate(oracle["args"]["path"], variables))
            deadline = time.monotonic() + oracle["args"].get("wait_seconds", 10)
            while not path.exists() and time.monotonic() < deadline:
                time.sleep(0.2)
            return json.loads(path.read_text())
        if self.server_oracle is None:
            raise RuntimeError(f"oracle {kind} needs a server oracle backend (--oracle-cmd or --ts-inspector-port)")
        return self.server_oracle(kind, interpolate(oracle.get("args", {}), variables))


class Runner:
    def __init__(self, endpoints: Endpoints, oracle: OracleBackend, variables: dict[str, Any]):
        self.endpoints = endpoints
        self.oracle = oracle
        self.variables = variables

    def _apply_oracles(self, case: dict) -> None:
        for oracle in case.get("oracles", []):
            value = self.oracle.run(oracle, self.variables)
            for name, selector in oracle.get("save", {}).items():
                self.variables[name] = extract(value, selector) if selector else value

    def _apply_captures(self, case: dict, observed: Any) -> list[str]:
        errors = []
        for name, selector in case.get("capture", {}).items():
            try:
                self.variables[name] = extract(observed, selector)
            except (KeyError, IndexError, TypeError, StopIteration) as error:
                errors.append(f"capture {name} <- {selector} failed: {error!r}")
        return errors

    def execute(self, case: dict) -> Any:
        if case.get("delay_before"):
            time.sleep(case["delay_before"])
        self._apply_oracles(case)
        request = interpolate(case.get("request", {}), self.variables)
        if case["transport"] == "tls-certificate":
            return execute_tls_certificate(self.endpoints)
        if case["transport"] == "ws":
            return execute_ws(self.endpoints, request, interpolate(case.get("steps", []), self.variables))
        return execute_http(self.endpoints, request)

    def record(self, case: dict, substitutable: list[str]) -> dict:
        observed = self.execute(case)
        capture_errors = self._apply_captures(case, observed)
        if capture_errors:
            raise RuntimeError(f"{case['id']}: {capture_errors}; observed={json.dumps(observed, ensure_ascii=False)[:2000]}")
        expect = Normalizer(self.variables, substitutable).normalize(_without_detail(observed), case.get("volatile", {}))
        golden = {key: value for key, value in case.items() if key not in ("volatile",)}
        golden["expect"] = expect
        return golden

    def replay(self, golden: dict) -> CaseResult:
        try:
            observed = self.execute(golden)
        except Exception as error:  # noqa: BLE001 - 재생은 케이스 하나의 실패로 멈추지 않는다
            return CaseResult(golden["id"], False, [f"execution failed: {error!r}"])
        errors = match(golden["expect"], _without_detail(observed), self.variables)
        errors.extend(self._apply_captures(golden, observed))
        return CaseResult(golden["id"], not errors, errors, observed)


def _without_detail(observed: Any) -> Any:
    """끊김의 세부 원인(RST인지 EOF인지)은 타이밍에 따라 바뀌어 비교에서 뺀다"""
    if isinstance(observed, dict):
        return {key: _without_detail(value) for key, value in observed.items() if key != "detail"}
    if isinstance(observed, list):
        return [_without_detail(item) for item in observed]
    return observed


def wait_for_socket(host: str, port: int, timeout: float) -> bool:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with socket.create_connection((host, port), timeout=1):
                return True
        except OSError:
            time.sleep(0.3)
    return False
