#!/usr/bin/env python3
"""GPUI(Rust) 앱을 states.yaml 상태로 띄워 TS 기준 이미지와 같은 방법으로 찍고 비교한다.

TS 드라이버(`parity/tools/screens/ts-driver`)와 같은 Xvfb 화면 크기·`ffmpeg -f x11grab` 명령·fixture 생성기·고정 글꼴 설정을
`parity/tools/screens/capture.py`에서 가져다 쓴다. 앱은 `kanvibe --qa`로 띄우고, 단계는 QA 제어 소켓(한 줄 JSON)으로 보낸다.
앱에 넘기는 결정성 입력(고정 시각, 네트워크 응답 표, 고장 목록, 소켓 경로)은 모두 실행 인자이고, 환경은 capture.py가 만드는
격리 환경을 앱 프로세스에만 준다.

예:
  python3 parity/tools/screens-rust/capture_rust.py --only SCR-NF-01,SCR-BOARD-01 --diff
"""
from __future__ import annotations

import argparse
import json
import socket
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parents[1] / "screens"
sys.path.insert(0, str(TOOLS_DIR))

import capture  # noqa: E402  (fixture·Xvfb·x11grab·글꼴 설정을 TS 캡처와 공유한다)
from diff import compare_images  # noqa: E402

REPO_ROOT = TOOLS_DIR.parents[2]
DEFAULT_ROOT = Path("/tmp/kanvibe-parity-rust")
# TS 기준 캡처(:93)와 동시에 돌 수 있도록 다른 화면 번호를 쓴다
DEFAULT_DISPLAY = 94
# 기준 이미지의 Electron 창 내용은 Xvfb 화면보다 오른쪽·아래가 1px 작다(마지막 열·행이 검은 루트 창).
# 같은 픽셀을 얻으려면 GPUI 창도 그 크기로 연다.
VIEWPORT_INSET = 1
SERVICE_RESULTS_PATH = REPO_ROOT / "parity/fixtures/service-results.json"


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


def resolve_fault(fault: dict) -> dict:
    """`value_ref`를 service-results.json 값으로 풀어 앱이 그대로 쓰게 한다."""
    resolved = dict(fault)
    if "value_ref" in resolved:
        values = json.loads(SERVICE_RESULTS_PATH.read_text(encoding="utf-8"))["values"]
        resolved["value"] = values[resolved.pop("value_ref")]
    return resolved


def target_center(client: ControlClient, target) -> tuple[float, float, dict]:
    """대상의 창 좌표 사각형과 가운데."""
    bounds = client.send({"cmd": "bounds", "target": target})["bounds"]
    return bounds["x"] + bounds["width"] / 2, bounds["y"] + bounds["height"] / 2, bounds


def drag_hold(client: ControlClient, value: dict) -> None:
    """TS 드라이버 `drag_hold`: 대상을 집어 4px 끌고 200ms 쉰 뒤 목표 위로 12단계에 걸쳐 옮기고 놓지 않는다."""
    _, _, source = target_center(client, value["from"])
    target_x, _, target = target_center(client, value["to"])
    start_x = source["x"] + source["width"] / 2
    start_y = source["y"] + min(20, source["height"] / 2)
    client.send({"cmd": "mouse_move", "x": start_x, "y": start_y})
    client.send({"cmd": "mouse_down"})
    for step in (1, 2):
        client.send({"cmd": "mouse_move", "x": start_x + 2 * step, "y": start_y + 2 * step})
    time.sleep(0.2)
    end_y = target["y"] + value["to"].get("offset_y", 40) if isinstance(value["to"], dict) else target["y"] + 40
    from_x, from_y = start_x + 4, start_y + 4
    for step in range(1, 13):
        client.send({"cmd": "mouse_move", "x": from_x + (target_x - from_x) * step / 12,
                     "y": from_y + (end_y - from_y) * step / 12})


def run_step(client: ControlClient, step: dict) -> None:
    kind, value = next(iter(step.items()))
    if kind == "wait":
        time.sleep(value / 1000)
    elif kind == "wait_for":
        timeout = value.get("timeout") if isinstance(value, dict) else None
        client.send({"cmd": "wait_for", "target": value, "timeout_ms": timeout})
    elif kind in ("click", "double_click", "right_click"):
        client.send({"cmd": "click", "target": value, "button": "right" if kind == "right_click" else "left",
                     "click_count": 2 if kind == "double_click" else 1})
    elif kind == "hover":
        client.send({"cmd": "hover", "target": value})
    elif kind == "type":
        client.send({"cmd": "type", "text": value})
    elif kind == "press":
        for keys in value if isinstance(value, list) else [value]:
            client.send({"cmd": "key", "keys": keys})
    elif kind == "fault":
        client.send({"cmd": "fault", "fault": resolve_fault(value)})
    elif kind == "clear_fault":
        client.send({"cmd": "clear_fault", "target": value})
    elif kind == "navigate":
        client.send({"cmd": "navigate", "route": value})
    elif kind == "mouse_move":
        client.send({"cmd": "mouse_move", "x": value["x"], "y": value["y"]})
    elif kind in ("mouse_down", "mouse_up"):
        client.send({"cmd": kind})
    elif kind == "board_event":
        client.send({"cmd": "board_event", "event": value})
    elif kind == "fill":
        # Playwright `fill`: 입력란에 포커스를 주고 값을 통째로 바꾼다(키를 치지 않는다)
        client.send({"cmd": "fill", "target": {k: v for k, v in value.items() if k != "value"},
                     "value": value["value"]})
    elif kind == "focus":
        client.send({"cmd": "focus", "target": value})
    elif kind == "scroll_into_view":
        client.send({"cmd": "scroll_into_view", "target": value})
    elif kind == "main_send":
        # Electron main이 창에 보내던 메시지(`webContents.send(channel, payload)`)
        client.send({"cmd": "main_send", "channel": value["channel"], "payload": value["payload"]})
    elif kind == "drag_hold":
        drag_hold(client, value)
    else:
        raise RuntimeError(f"step not supported by the Rust app yet: {kind}")


def app_command(args: argparse.Namespace, state: dict, fixture: dict, job_dir: Path) -> list[str]:
    faults_path = job_dir / "faults.json"
    faults_path.write_text(json.dumps([resolve_fault(fault) for fault in state.get("faults") or []],
                                      ensure_ascii=False), encoding="utf-8")
    fixed_now = datetime.fromtimestamp(capture.FIXED_NOW_EPOCH_MS / 1000, tz=timezone.utc)
    width = state["window"]["width"] - VIEWPORT_INSET
    height = state["window"]["height"] - VIEWPORT_INSET
    return [
        str(args.app), "--qa",
        "--data-dir", fixture["appdata"],
        "--fixture-root", fixture["root"],
        "--fixed-now", fixed_now.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "--network-responses", str(capture.FIXTURES_DIR / "network" / f"{state.get('network', 'default')}.json"),
        "--faults", str(faults_path),
        "--control-socket", str(job_dir / "control.sock"),
        "--route", state["route"],
        "--window-size", f"{width}x{height}",
        *(["--platform", "mac"] if state.get("platform") == "mac" else []),
        # TS 드라이버 `disable_javascript`: 화면 코드가 돌기 전 정적 스켈레톤에서 멈춘다
        *(["--hold-first-frame"] if state.get("disable_javascript") else []),
        # TS 드라이버 `hold_requests`: 정규식에 맞는 lazy 라우트 번들을 끝내지 않는다
        *[argument for pattern in state.get("hold_requests") or [] for argument in ("--hold-request", pattern)],
    ]


def capture_state(args: argparse.Namespace, state: dict, output_path: Path) -> None:
    work_root = Path(args.root).resolve()
    fixture_root = work_root / "fixture"
    overrides = {"theme_preference": state["theme"], **{k: str(v) for k, v in (state.get("settings") or {}).items()}}
    fixture = capture.build_fixture(fixture_root, state.get("fixture", "full"), overrides, state.get("fixture_extras") or [])
    fontconfig_path = capture.write_pinned_fontconfig(work_root / "fontconfig")
    environment = capture.build_app_environment(fixture, fixture_root, args.display, fontconfig_path)
    environment.update(dict(item.split("=", 1) for item in args.app_env))
    job_dir = work_root / "job"
    job_dir.mkdir(parents=True, exist_ok=True)
    # 지난 실행의 소켓 파일이 남아 있으면 새 앱이 열기 전에 접속을 시도해 거부당한다
    (job_dir / "control.sock").unlink(missing_ok=True)
    width, height = state["window"]["width"], state["window"]["height"]
    log_dir = work_root / "logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    xvfb = capture.start_xvfb(args.display, width, height, log_dir / "xvfb.log")
    app_log = (log_dir / f"{state['image_id']}.app.log").open("w")
    app = None
    try:
        app = subprocess.Popen(app_command(args, state, fixture, job_dir), env=environment,
                               stdout=app_log, stderr=subprocess.STDOUT)
        client = ControlClient(job_dir / "control.sock")
        client.send({"cmd": "wait_idle"})
        # TS 기준 이미지는 Playwright가 포커스를 준 창에서 찍었다. 창 관리자가 없는 Xvfb에서는 앱이 직접 포커스를 잡는다
        client.send({"cmd": "activate_window"})
        for step in state.get("steps") or []:
            run_step(client, step)
            # 다음 단계가 이번 단계로 바뀐 화면(새로 뜬 메뉴 항목 등)을 찾도록 프레임을 기다린다
            client.send({"cmd": "wait_idle"})
        if not state.get("keep_mouse"):
            client.send({"cmd": "park_pointer"})
        time.sleep(state.get("settle_ms", 1500) / 1000)
        client.send({"cmd": "report_ready"})
        capture.grab_window(args.display, width, height, output_path)
        client.send({"cmd": "quit"})
        app.wait(timeout=10)
    finally:
        capture.stop_process(app)
        app_log.close()
        capture.kill_fixture_multiplexers(environment)
        capture.kill_leftover_processes(fixture_root)
        capture.stop_process(xvfb)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--only", required=True, help="쉼표로 구분한 상태 ID 또는 이미지 ID")
    parser.add_argument("--app", default=str(REPO_ROOT / "target/debug/kanvibe"))
    parser.add_argument("--root", default=str(DEFAULT_ROOT))
    parser.add_argument("--out", default=None, help="후보 이미지 디렉터리(기본 <root>/candidate)")
    parser.add_argument("--display", type=int, default=DEFAULT_DISPLAY)
    parser.add_argument("--baseline", default=str(capture.SCREENS_DIR / "baseline"))
    parser.add_argument("--diff", action="store_true", help="기준 이미지와 비교하고 차이 그림을 남긴다")
    parser.add_argument("--app-env", action="append", default=[], metavar="KEY=VALUE",
                        help="렌더링 실험용으로 앱 프로세스에만 더할 환경 값(예: ZED_FONTS_GAMMA=1.0)")
    args = parser.parse_args()

    _, states = capture.load_states(capture.SCREENS_DIR / "states.yaml")
    wanted = set(args.only.split(","))
    states = [state for state in states if state["id"] in wanted or state["image_id"] in wanted]
    output_dir = Path(args.out or Path(args.root) / "candidate")
    output_dir.mkdir(parents=True, exist_ok=True)
    results = {}
    for state in states:
        image_path = output_dir / f"{state['image_id']}.png"
        try:
            capture_state(args, state, image_path)
        except Exception as error:  # noqa: BLE001 - 한 상태가 실패해도 나머지를 찍는다
            results[state["image_id"]] = {"error": str(error)[-600:]}
            continue
        baseline_path = Path(args.baseline) / image_path.name
        if args.diff and baseline_path.exists():
            results[state["image_id"]] = compare_images(baseline_path, image_path, output_dir / f"{state['image_id']}.diff.png")
        else:
            results[state["image_id"]] = {"captured": str(image_path)}
    print(json.dumps(results, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
