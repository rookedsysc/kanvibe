#!/usr/bin/env python3
"""states.yaml의 각 상태를 Xvfb에서 띄운 앱으로 재현하고 ffmpeg x11grab으로 찍는다.

앱 종류와 상관없이 같은 경로로 찍는다. 앱을 띄우고 단계를 실행하는 일만 드라이버(--driver)가 맡는다.
  ts    : parity/tools/screens/ts-driver/driver.cjs (Electron + Playwright, TS 제거 때 삭제)
  그 밖 : --driver-command 로 넘긴 명령. job JSON 경로를 마지막 인자로 받고, 준비되면 stdout에
          {"event":"ready"}를 쓰고, stdin으로 "captured"를 받으면 앱을 닫고 {"event":"done"}을 쓴다.

창 관리자 없이 Xvfb 화면 크기를 창 크기와 같게 두고, 창은 (0,0)에 정확한 크기로 놓는다.
사용자 tmux·셸·HOME은 건드리지 않는다. 앱 프로세스에만 격리 HOME, XDG_*, TMUX_TMPDIR, FONTCONFIG_FILE을 넘긴다.

예:
  python3 parity/tools/screens/capture.py --runs 2                      # 전체 130장, 두 번씩
  python3 parity/tools/screens/capture.py --only SCR-BOARD-04 --runs 1  # 한 상태만
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path

import yaml

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parents[2]
PARITY_DIR = REPO_ROOT / "parity"
FIXTURES_DIR = PARITY_DIR / "fixtures"
SCREENS_DIR = PARITY_DIR / "screens"
sys.path.insert(0, str(FIXTURES_DIR))
sys.path.insert(0, str(TOOLS_DIR))

from build_fixture import FIXED_NOW_EPOCH_MS, build_fixture  # noqa: E402
from diff import compare_images  # noqa: E402
from fontconfig import write_pinned_fontconfig  # noqa: E402

# 경로가 화면에 찍히므로 사용자 이름·HOME과 상관없는 고정 경로를 쓴다(CI·다른 머신·Rust 캡처에서도 같아야 한다)
DEFAULT_ROOT = Path("/tmp/kanvibe-parity")
DISPLAY_NUMBER = 93
HOOK_PORT = 47736
MOBILE_TLS_PORT_OFFSET = 10


def multiplexer_bin_dirs() -> list[str]:
    """tmux·zellij·herdr가 깔린 디렉터리만 앱 PATH에 더한다. 머신마다 위치가 다르므로 실행 시 찾는다."""
    dirs = []
    for tool in ("tmux", "zellij"):
        found = shutil.which(tool)
        if found and os.path.dirname(found) not in dirs:
            dirs.append(os.path.dirname(found))
    return dirs


def load_states(states_path: Path) -> tuple[dict, list[dict]]:
    document = yaml.safe_load(states_path.read_text(encoding="utf-8"))
    defaults = document["defaults"]
    expanded = []
    for state in document["states"]:
        themes = state.get("themes", ["dark"])
        for theme in themes:
            image_id = state["id"] if theme == "dark" else f"{state['id']}--light"
            expanded.append({**defaults, **state, "theme": theme, "image_id": image_id})
    return document, expanded


def is_port_free(port: int) -> bool:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            probe.bind(("0.0.0.0", port))
        except OSError:
            return False
    return True


def wait_for_ports(ports: list[int], timeout_s: float = 20) -> None:
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        if all(is_port_free(port) for port in ports):
            return
        time.sleep(0.25)
    raise RuntimeError(f"fixed ports still busy: {ports}")


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
    """이 실행의 TMUX_TMPDIR에 있는 tmux 서버만 끈다. 사용자 기본 소켓은 건드리지 않는다."""
    tmux_environment = {"PATH": environment["PATH"], "HOME": environment["HOME"], "TMUX_TMPDIR": environment["TMUX_TMPDIR"]}
    subprocess.run(["tmux", "kill-server"], env=tmux_environment, capture_output=True)


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


def driver_command(args: argparse.Namespace) -> list[str]:
    if args.driver == "ts":
        return ["node", str(TOOLS_DIR / "ts-driver" / "driver.cjs")]
    return args.driver_command.split()


def capture_state(state: dict, run_index: int, args: argparse.Namespace, work_root: Path, output_path: Path) -> dict:
    fixture_root = work_root / "fixture"
    overrides = {"theme_preference": state["theme"], **{k: str(v) for k, v in (state.get("settings") or {}).items()}}
    fixture = build_fixture(fixture_root, state.get("fixture", "full"), overrides, state.get("fixture_extras") or [])
    fontconfig_path = write_pinned_fontconfig(work_root / "fontconfig", use_system_fonts=args.system_fonts)
    width, height = state["window"]["width"], state["window"]["height"]
    environment = build_app_environment(fixture, fixture_root, DISPLAY_NUMBER, fontconfig_path)
    network_fixture = FIXTURES_DIR / "network" / f"{state.get('network', 'default')}.json"

    # 단계 안의 {fixture_root}는 이 실행의 fixture 경로로 바꾼다(가짜 CLI 실행 명령 등)
    state = json.loads(json.dumps(state, ensure_ascii=False).replace("{fixture_root}", str(fixture_root)))
    job = {
        "state": state,
        "env": environment,
        "window": {"width": width, "height": height},
        "fixedNowMs": FIXED_NOW_EPOCH_MS,
        "hookPort": HOOK_PORT,
        "networkFixturePath": str(network_fixture),
        "settleMs": state.get("settle_ms", 1500),
        "fontAuditPath": str(output_path.with_suffix(".fonts.json")) if args.font_audit else None,
    }
    job_path = work_root / "job.json"
    job_path.write_text(json.dumps(job, ensure_ascii=False, indent=2), encoding="utf-8")

    log_dir = work_root / "logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    wait_for_ports([HOOK_PORT, HOOK_PORT + MOBILE_TLS_PORT_OFFSET])
    xvfb = start_xvfb(DISPLAY_NUMBER, width, height, log_dir / "xvfb.log")
    driver_log = (log_dir / f"{state['image_id']}.run{run_index}.driver.log").open("w")
    driver = None
    started = time.monotonic()
    try:
        driver = subprocess.Popen(
            [*driver_command(args), str(job_path)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=driver_log, text=True,
            env={"PATH": os.environ["PATH"], "HOME": os.environ["HOME"], "DISPLAY": f":{DISPLAY_NUMBER}"},
        )
        ready = False
        for line in driver.stdout:
            event = json.loads(line)
            if event["event"] == "ready":
                grab_window(DISPLAY_NUMBER, width, height, output_path)
                driver.stdin.write("captured\n")
                driver.stdin.flush()
                ready = True
            elif event["event"] == "error":
                raise RuntimeError(event["message"])
            elif event["event"] == "done":
                break
        driver.wait(timeout=60)
        if not ready:
            raise RuntimeError("driver exited before ready")
        return {"ok": True, "seconds": round(time.monotonic() - started, 1)}
    finally:
        stop_process(driver)
        driver_log.close()
        kill_fixture_multiplexers(environment)
        kill_leftover_processes(fixture_root)
        stop_process(xvfb)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--states", default=str(SCREENS_DIR / "states.yaml"))
    parser.add_argument("--out", default=str(SCREENS_DIR / "baseline"))
    parser.add_argument("--root", default=str(DEFAULT_ROOT), help="fixture 작업 루트. 경로가 화면에 찍히므로 바꾸면 기준 이미지와 달라진다")
    parser.add_argument("--only", default="", help="쉼표로 구분한 상태 ID 또는 이미지 ID")
    parser.add_argument("--runs", type=int, default=2)
    parser.add_argument("--driver", default="ts")
    parser.add_argument("--driver-command", default="")
    parser.add_argument("--system-fonts", action="store_true", help="고정 글꼴 대신 시스템 글꼴 전체를 쓴다(글꼴 조사용)")
    parser.add_argument("--font-audit", action="store_true", help="이미지 옆에 <id>.fonts.json(실제 쓰인 글꼴)을 남긴다")
    parser.add_argument("--keep-going", action="store_true")
    args = parser.parse_args()

    _, states = load_states(Path(args.states))
    if args.only:
        wanted = set(args.only.split(","))
        states = [state for state in states if state["id"] in wanted or state["image_id"] in wanted]

    work_root = Path(args.root).resolve()
    output_dir = Path(args.out)
    runs_dir = work_root / "runs"
    output_dir.mkdir(parents=True, exist_ok=True)
    manifest_path = output_dir / "manifest.json"
    manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else {"images": {}}
    manifest["meta"] = {
        # 기준 이미지가 나타내는 TS 앱 소스의 마지막 커밋(저장소 HEAD가 아니라 앱 소스를 바꾼 커밋)
        "ts_app_commit": subprocess.run(
            ["git", "-C", str(REPO_ROOT), "log", "-1", "--format=%H", "--", "src", "electron", "index.html", "messages", "package.json"],
            capture_output=True, text=True).stdout.strip(),
        "driver": args.driver,
        "fixed_now_ms": FIXED_NOW_EPOCH_MS,
        "fixture_root": str(Path(args.root).resolve() / "fixture"),
        "capture": "ffmpeg -f x11grab -draw_mouse 0, Xvfb 24bit, no window manager, window at (0,0)",
        "determinism_rule": "two independent launches per image; deterministic when differing pixels < 0.1%",
    }

    for state in states:
        image_id = state["image_id"]
        run_paths = []
        entry = {"state_id": state["id"], "theme": state["theme"], "window": state["window"]}
        try:
            for run_index in range(1, args.runs + 1):
                run_path = runs_dir / f"run{run_index}" / f"{image_id}.png"
                result = capture_state(state, run_index, args, work_root, run_path)
                run_paths.append(run_path)
                print(f"{image_id} run{run_index} ok {result['seconds']}s", flush=True)
            shutil.copyfile(run_paths[0], output_dir / f"{image_id}.png")
            if len(run_paths) >= 2:
                # 같은 앱을 두 번 찍은 결정성 판정이라 색 허용치 없이 비교한다
                comparison = compare_images(run_paths[0], run_paths[1], strict=True)
                entry["determinism"] = {
                    "runs": len(run_paths),
                    "diff_pixels": comparison["diff_pixels"],
                    "diff_ratio": comparison["diff_ratio"],
                    "deterministic": comparison["diff_ratio"] < 0.001,
                }
                print(f"{image_id} determinism diff_ratio={comparison['diff_ratio']:.6f}", flush=True)
            entry["status"] = "captured"
        except Exception as error:  # noqa: BLE001 - 상태 하나가 실패해도 나머지는 계속 찍는다
            entry["status"] = "failed"
            entry["error"] = str(error)[-2000:]
            print(f"{image_id} FAILED: {str(error)[-600:]}", flush=True)
            if not args.keep_going:
                manifest["images"][image_id] = entry
                manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True))
                raise
        manifest["images"][image_id] = entry
        manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
