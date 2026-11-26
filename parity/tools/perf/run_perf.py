#!/usr/bin/env python3
"""Rust(GPUI) 릴리스 앱의 성능을 고정 조건에서 재고 JSON·Markdown 보고서를 남긴다.

고정 조건: 스크린샷과 같은 fixture(`parity/fixtures/build_fixture.py` full, 다크 테마), 같은 고정 글꼴 설정(FONTCONFIG_FILE),
같은 Xvfb 화면(1600x1000x24, 96dpi, 창 관리자 없음), 같은 창 크기(1600x1000), 같은 격리 환경(HOME·XDG_*·TMUX_TMPDIR),
측정마다 새 fixture와 새 프로세스. 앱은 자동 QA 하네스 모드(`--qa-harness`)에 hook 서버를 켠다(`--kanvibe-hook-port`). 첫 회는 디스크 캐시
워밍업으로 버린다. TS(Electron) 앱을 같은 조건으로 재던 경로는 TS 코드와 함께 지웠다(커밋 `a73f3498`까지 git 기록에 있다).

지표와 방법
- 콜드 스타트: 하네스가 프로세스를 띄우기 직전 시각부터 `first_board_paint` 표식까지. 표식은 Todo 열 카드가 처음 그려진
  프레임의 다음 프레임 시작에서 나온다. `kanvibe --qa-harness --perf-markers`(qa/perf_markers.rs)가 QA 대상 목록으로 판단한다.
  "변화가 그려진 프레임 다음 프레임의 시작"이라 프레임 간격(약 16ms)만큼 양자화된다.
- 메모리: 첫 페인트 +settle초(기본 10초) 뒤 앱 프로세스 트리 전체의 `/proc/<pid>/smaps_rollup` Pss 합(참고로 Rss 합).
  tmux 서버는 데몬이라 트리 밖이고 세지 않는다.
- 유휴 CPU: 그 직후 idle초(기본 60초) 동안 트리 전체의 utime+stime 증가량 ÷ CLK_TCK ÷ 시간(한 코어 기준 %).
  트리는 1초마다 다시 모으고, 도중에 생긴 프로세스는 0부터 센다.
- 키→페인트(보드): 찾기 바를 연 뒤(Ctrl+F) `a`와 BackSpace를 번갈아 xdotool로 넣고, 키 직전 시각부터 `key_frame`
  (키 처리 뒤 첫 프레임이 그려진 다음 프레임 시작)까지. 앱 안 수신 시각(`key_received`)부터의
  값도 함께 남긴다(xdotool 프로세스 시작 비용을 뺀 값).
- 키→페인트(터미널): 터미널 Task 화면에서 셸 프롬프트에 `x`를 넣고 `key_text_change`(터미널 줄 글이 바뀐 프레임이 그려진
  다음 프레임 시작)까지. PTY·tmux 왕복이 들어 있고 fixture tmux 세션을 쓴다.
모든 시각은 유닉스 epoch ns(CLOCK_REALTIME)다. 앱과 하네스가 같은 시계를 읽으므로 측정 중 시계 조정이 없으면 차이가 맞다.

캐시: 기본은 콜드다. 세션마다 새 XDG_CACHE_HOME(Mesa 셰이더 캐시 등)과 새 fontconfig 캐시를 쓴다. `--warm-cache`면
실행 전체가 격리 작업 디렉터리 안의 캐시 디렉터리 하나(XDG_CACHE_HOME, 고정 fontconfig 설정과 그 캐시)를 함께 쓰고,
워밍업 회(최소 1회)가 그 캐시를 채운 뒤 본 측정을 한다. fixture(DB·HOME·설정)는 두 모드 모두 세션마다 새로 만든다.

부하 확인: 1분 load average가 2를 넘으면(다른 빌드가 돌고 있으면) 본 측정을 거부한다. `--smoke`는 하네스 동작만 확인한다.

예:
  python3 parity/tools/perf/run_perf.py --smoke                  # 1회, 짧은 대기, 하네스 확인용
  python3 parity/tools/perf/run_perf.py --runs 10                # 본 측정(조용한 머신에서)
  python3 parity/tools/perf/run_perf.py --runs 10 --warm-cache   # 캐시를 채운 상태의 본 측정
결과: qa-results/roky-harness/perf/<UTC 시각>-<smoke|full>[-warm]/result.json, report.md, logs/
"""
from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import signal
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from collections import defaultdict
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parents[2]
sys.path.insert(0, str(TOOLS_DIR.parent / "screens"))
sys.path.insert(0, str(TOOLS_DIR.parent / "app-launch"))

import capture  # noqa: E402  (fixture·글꼴·Xvfb·격리 환경을 스크린샷과 공유한다)
from check_release_launch import free_display, free_hook_port  # noqa: E402

DEFAULT_RUST_APP = REPO_ROOT / "target/release/kanvibe"
OUTPUT_ROOT = REPO_ROOT / "qa-results/roky-harness/perf"
WINDOW_SIZE = (1600, 1000)
BOARD_ROUTE = "/ko"
TERMINAL_ROUTE = "/ko/task/22222222-2222-4222-8222-000000000010"
TERMINAL_CLICK_POINT = (800, 500)
MARKER_PREFIX = "KANVIBE_PERF "
MAX_LOAD_AVERAGE = 2.0
CLOCK_TICKS_PER_SECOND = os.sysconf("SC_CLK_TCK")


@dataclass
class Settings:
    runs: int
    warmup: int
    settle_s: float
    idle_s: float
    key_count: int
    rust_app: Path
    # 세션 사이에 이어 쓰는 캐시 디렉터리. None이면 세션마다 새 캐시(콜드)
    warm_cache_dir: Path | None


@dataclass
class MarkerLog:
    """앱 stdout을 읽어 표식을 모으고 나머지 줄은 로그 파일에 남긴다"""
    log_path: Path
    markers: list[tuple[str, int]] = field(default_factory=list)
    condition: threading.Condition = field(default_factory=threading.Condition)

    def follow(self, stream) -> None:
        with self.log_path.open("w", encoding="utf-8") as log:
            for raw_line in stream:
                line = raw_line.decode("utf-8", "replace")
                log.write(line)
                if line.startswith(MARKER_PREFIX):
                    _, name, epoch_ns = line.split()
                    with self.condition:
                        self.markers.append((name, int(epoch_ns)))
                        self.condition.notify_all()

    def wait_for(self, names: set[str], after_ns: int, timeout_s: float) -> tuple[str, int]:
        deadline = time.monotonic() + timeout_s
        with self.condition:
            while True:
                found = [(name, at) for name, at in self.markers if name in names and at >= after_ns]
                if found:
                    return found[0]
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError(f"marker {sorted(names)} not seen within {timeout_s}s")
                self.condition.wait(remaining)


@dataclass
class LaunchedApp:
    process: subprocess.Popen
    markers: MarkerLog
    environment: dict[str, str]
    fixture_root: Path
    spawn_ns: int
    xvfb: subprocess.Popen
    stderr_log: object


# ───────────── 앱 띄우기 ─────────────

def fixed_now_iso() -> str:
    fixed_now = datetime.fromtimestamp(capture.FIXED_NOW_EPOCH_MS / 1000, tz=timezone.utc)
    return fixed_now.strftime("%Y-%m-%dT%H:%M:%SZ")


def rust_command(app: Path, fixture: dict, route: str, hook_port: int) -> list[str]:
    return [
        str(app), "--qa-harness", "--perf-markers",
        "--data-dir", fixture["appdata"],
        "--fixture-root", fixture["root"],
        "--fixed-now", fixed_now_iso(),
        "--network-responses", str(capture.FIXTURES_DIR / "network" / "default.json"),
        "--route", route,
        "--window-size", f"{WINDOW_SIZE[0]}x{WINDOW_SIZE[1]}",
        "--kanvibe-hook-port", str(hook_port),
    ]


def pinned_fontconfig_path(session_dir: Path, warm_cache_dir: Path | None) -> Path:
    """웜 캐시면 고정 글꼴 설정을 한 번만 만들어 이어 쓴다. 다시 만들면 글꼴 디렉터리가 바뀌어 fontconfig 캐시가 무효가 된다"""
    if warm_cache_dir is None:
        return capture.write_pinned_fontconfig(session_dir / "fontconfig")
    shared_config_path = warm_cache_dir / "fontconfig" / "fonts.conf"
    if shared_config_path.exists():
        return shared_config_path
    return capture.write_pinned_fontconfig(shared_config_path.parent)


def launch_app(route: str, session_dir: Path, settings: Settings, fixture_overrides: dict) -> LaunchedApp:
    fixture_root = session_dir / "fixture"
    fixture = capture.build_fixture(fixture_root, "full", {"theme_preference": "dark", **fixture_overrides})
    fontconfig_path = pinned_fontconfig_path(session_dir, settings.warm_cache_dir)
    display = free_display()
    environment = capture.build_app_environment(fixture, fixture_root, display, fontconfig_path)
    if settings.warm_cache_dir is not None:
        environment["XDG_CACHE_HOME"] = str(settings.warm_cache_dir / "xdg-cache")
    hook_port = free_hook_port()
    command = rust_command(settings.rust_app, fixture, route, hook_port)
    logs_dir = session_dir / "logs"
    logs_dir.mkdir(parents=True, exist_ok=True)
    xvfb = capture.start_xvfb(display, *WINDOW_SIZE, logs_dir / "xvfb.log")
    markers = MarkerLog(logs_dir / "app.stdout.log")
    stderr_log = (logs_dir / "app.stderr.log").open("w")
    spawn_ns = time.time_ns()
    process = subprocess.Popen(command, env=environment, cwd=REPO_ROOT, stdout=subprocess.PIPE,
                               stderr=stderr_log, start_new_session=True)
    threading.Thread(target=markers.follow, args=(process.stdout,), daemon=True).start()
    return LaunchedApp(process, markers, environment, fixture_root, spawn_ns, xvfb, stderr_log)


def stop_app(launched: LaunchedApp) -> None:
    process = launched.process
    if process.poll() is None:
        process.send_signal(signal.SIGTERM)
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            pass
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait()
    launched.stderr_log.close()
    capture.kill_fixture_multiplexers(launched.environment)
    capture.kill_leftover_processes(launched.fixture_root)
    capture.stop_process(launched.xvfb)


# ───────────── 프로세스 트리 측정 ─────────────

def read_stat_fields(pid: int) -> list[str] | None:
    """`/proc/<pid>/stat`에서 실행 파일 이름(괄호) 뒤 필드. 사라진 프로세스면 None"""
    try:
        text = Path(f"/proc/{pid}/stat").read_text()
    except OSError:
        return None
    return text[text.rindex(")") + 2:].split()


def process_tree(root_pid: int) -> list[int]:
    children = defaultdict(list)
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        fields = read_stat_fields(int(entry))
        if fields:
            children[int(fields[1])].append(int(entry))
    tree, pending = [], [root_pid]
    while pending:
        pid = pending.pop()
        tree.append(pid)
        pending.extend(children.get(pid, []))
    return tree


def measure_tree_memory(root_pid: int) -> dict:
    totals = {"pss_kib": 0, "rss_kib": 0, "process_count": 0}
    for pid in process_tree(root_pid):
        try:
            rollup = Path(f"/proc/{pid}/smaps_rollup").read_text()
        except OSError:
            continue
        totals["process_count"] += 1
        for line in rollup.splitlines():
            key, _, value = line.partition(":")
            if key in ("Pss", "Rss"):
                totals[f"{key.lower()}_kib"] += int(value.split()[0])
    return {**totals, "pss_mib": round(totals["pss_kib"] / 1024, 1), "rss_mib": round(totals["rss_kib"] / 1024, 1)}


def cpu_ticks(pid: int) -> int | None:
    fields = read_stat_fields(pid)
    return int(fields[11]) + int(fields[12]) if fields else None


def measure_idle_cpu(root_pid: int, duration_s: float) -> dict:
    first_ticks = {pid: ticks for pid in process_tree(root_pid) if (ticks := cpu_ticks(pid)) is not None}
    last_ticks = dict(first_ticks)
    started = time.monotonic()
    while time.monotonic() - started < duration_s:
        time.sleep(min(1.0, max(0.0, duration_s - (time.monotonic() - started))))
        for pid in process_tree(root_pid):
            ticks = cpu_ticks(pid)
            if ticks is not None:
                first_ticks.setdefault(pid, 0)
                last_ticks[pid] = ticks
    elapsed_s = time.monotonic() - started
    used_ticks = sum(last_ticks[pid] - first_ticks[pid] for pid in last_ticks)
    return {"cpu_percent": round(used_ticks / CLOCK_TICKS_PER_SECOND / elapsed_s * 100, 3),
            "cpu_seconds": round(used_ticks / CLOCK_TICKS_PER_SECOND, 3), "window_s": round(elapsed_s, 2)}


# ───────────── 입력 주입 ─────────────

def xdotool(environment: dict[str, str], *arguments: str) -> str:
    result = subprocess.run(["xdotool", *arguments], env={"DISPLAY": environment["DISPLAY"], "PATH": environment["PATH"]},
                            capture_output=True, text=True, timeout=10)
    return result.stdout


def focus_app_window(environment: dict[str, str]) -> None:
    """창 관리자가 없으므로 가장 큰 보이는 kanvibe 창에 입력 포커스를 직접 준다"""
    window_ids = xdotool(environment, "search", "--onlyvisible", "--class", "kanvibe").split()
    if not window_ids:
        raise RuntimeError("no visible kanvibe window to focus")

    def window_area(window_id: str) -> int:
        geometry = dict(line.split("=", 1) for line in xdotool(environment, "getwindowgeometry", "--shell", window_id).split())
        return int(geometry.get("WIDTH", 0)) * int(geometry.get("HEIGHT", 0))

    xdotool(environment, "windowfocus", max(window_ids, key=window_area))


def measure_key_latencies(launched: LaunchedApp, keys: list[str], marker_name: str) -> list[dict]:
    samples = []
    for key in keys:
        pressed_ns = time.time_ns()
        xdotool(launched.environment, "key", key)
        _, painted_ns = launched.markers.wait_for({marker_name}, pressed_ns, timeout_s=5)
        _, received_ns = launched.markers.wait_for({"key_received"}, pressed_ns, timeout_s=1)
        samples.append({"key": key, "from_inject_ms": round((painted_ns - pressed_ns) / 1e6, 3),
                        "from_app_receive_ms": round((painted_ns - received_ns) / 1e6, 3)})
        time.sleep(0.3)
    return samples


# ───────────── 세션 ─────────────

def run_board_session(session_dir: Path, settings: Settings) -> dict:
    launched = launch_app(BOARD_ROUTE, session_dir, settings, {})
    try:
        _, painted_ns = launched.markers.wait_for({"first_board_paint"}, launched.spawn_ns, timeout_s=120)
        result = {"cold_start_ms": round((painted_ns - launched.spawn_ns) / 1e6, 3)}
        time.sleep(settings.settle_s)
        result["memory"] = measure_tree_memory(launched.process.pid)
        result["idle_cpu"] = measure_idle_cpu(launched.process.pid, settings.idle_s)
        focus_app_window(launched.environment)
        xdotool(launched.environment, "mousemove", str(WINDOW_SIZE[0] - 1), str(WINDOW_SIZE[1] - 1))
        xdotool(launched.environment, "key", "ctrl+f")
        time.sleep(1.0)
        keys = ["a" if index % 2 == 0 else "BackSpace" for index in range(settings.key_count)]
        result["board_keys"] = measure_key_latencies(launched, keys, "key_frame")
        return result
    finally:
        stop_app(launched)


def run_terminal_session(session_dir: Path, settings: Settings) -> dict:
    launched = launch_app(TERMINAL_ROUTE, session_dir, settings, {"sidebar_default_collapsed": "true"})
    try:
        launched.markers.wait_for({"first_terminal_rows"}, launched.spawn_ns, timeout_s=120)
        # 셸 프롬프트가 다 그려지고 tmux 첫 출력이 끝날 때까지 둔다
        time.sleep(3.0)
        focus_app_window(launched.environment)
        xdotool(launched.environment, "mousemove", *map(str, TERMINAL_CLICK_POINT), "click", "1")
        time.sleep(0.5)
        return {"terminal_keys": measure_key_latencies(launched, ["x"] * settings.key_count, "key_text_change")}
    finally:
        stop_app(launched)


def run_session(scenario: str, session_dir: Path, settings: Settings) -> dict:
    runner = run_board_session if scenario == "board" else run_terminal_session
    started = time.monotonic()
    try:
        result = runner(session_dir, settings)
        result["ok"] = True
    except Exception as error:  # noqa: BLE001 - 한 세션이 실패해도 보고서에 원인을 남기고 다음 세션을 잰다
        result = {"ok": False, "error": f"{type(error).__name__}: {error}"}
    result["session_s"] = round(time.monotonic() - started, 1)
    print(f"[perf] {scenario} {session_dir.name}: {json.dumps(result)[:400]}", flush=True)
    return result


# ───────────── 집계·보고 ─────────────

def summarize(values: list[float]) -> dict | None:
    if not values:
        return None
    ordered = sorted(values)
    p90 = statistics.quantiles(ordered, n=10, method="inclusive")[8] if len(ordered) >= 2 else ordered[0]
    return {"n": len(ordered), "median": round(statistics.median(ordered), 3), "p90": round(p90, 3),
            "min": round(ordered[0], 3), "max": round(ordered[-1], 3)}


METRICS = {
    "cold_start_ms": ("콜드 스타트 → 첫 보드 페인트 (ms)", lambda runs: [r["cold_start_ms"] for r in runs if "cold_start_ms" in r]),
    "pss_mib": ("PSS 합, 페인트+settle (MiB)", lambda runs: [r["memory"]["pss_mib"] for r in runs if "memory" in r]),
    "idle_cpu_percent": ("유휴 CPU (한 코어 %)", lambda runs: [r["idle_cpu"]["cpu_percent"] for r in runs if "idle_cpu" in r]),
    "board_key_ms": ("키→페인트 보드, 주입부터 (ms)", lambda runs: [s["from_inject_ms"] for r in runs for s in r.get("board_keys", [])]),
    "board_key_app_ms": ("키→페인트 보드, 앱 수신부터 (ms)", lambda runs: [s["from_app_receive_ms"] for r in runs for s in r.get("board_keys", [])]),
    "terminal_key_ms": ("키→에코 페인트 터미널, 주입부터 (ms)", lambda runs: [s["from_inject_ms"] for r in runs for s in r.get("terminal_keys", [])]),
    "terminal_key_app_ms": ("키→에코 페인트 터미널, 앱 수신부터 (ms)", lambda runs: [s["from_app_receive_ms"] for r in runs for s in r.get("terminal_keys", [])]),
}


def summarize_sessions(sessions: list[dict]) -> dict:
    measured = [session for session in sessions if session.get("ok") and not session.get("warmup")]
    return {metric: summarize(extract(measured)) for metric, (_, extract) in METRICS.items()}


def median_cell(summary: dict | None) -> str:
    return f"{summary['median']} (n={summary['n']})" if summary else "-"


def p90_cell(summary: dict | None) -> str:
    return str(summary["p90"]) if summary else "-"


def write_markdown(report: dict, path: Path) -> None:
    meta = report["meta"]
    lines = [
        f"# KanVibe 성능 측정 ({meta['mode']})", "",
        f"- 시각: {meta['started_at']}, 커밋: `{meta['commit']}`, 머신: {meta['machine']}",
        f"- load average 시작/끝: {meta['load_average_start']} / {meta['load_average_end']}",
        f"- 캐시: {meta['cache']}",
        f"- 회수: {meta['runs']}회(워밍업 {meta['warmup']}회 제외), settle {meta['settle_s']}s, 유휴 {meta['idle_s']}s, 키 {meta['key_count']}개/회",
        "- 방법: `parity/tools/perf/run_perf.py` 머리말. GPU 없는 VM(lavapipe)이면 같은 머신 상대 비교만 뜻이 있다.",
        "", "| 지표 | 중앙값 | p90 |", "|---|---|---|",
    ]
    for metric, (label, _) in METRICS.items():
        summary = report["summary"].get(metric)
        lines.append(f"| {label} | {median_cell(summary)} | {p90_cell(summary)} |")
    failures = [f"- {s['scenario']} run{s['run']}: {s['error']}" for s in report["sessions"] if not s.get("ok")]
    if failures:
        lines += ["", "## 실패한 세션", *failures]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def machine_description() -> str:
    return f"{platform.node()} {platform.release()}, {os.cpu_count()} CPU"


def run_all(settings: Settings, output_dir: Path, work_root: Path) -> list[dict]:
    sessions = []
    for run_index in range(settings.warmup + settings.runs):
        for scenario in ("board", "terminal"):
            session_dir = work_root / f"{scenario}-run{run_index}"
            result = run_session(scenario, session_dir, settings)
            if (session_dir / "logs").exists():
                shutil.copytree(session_dir / "logs", output_dir / "logs" / session_dir.name, dirs_exist_ok=True)
            shutil.rmtree(session_dir, ignore_errors=True)
            sessions.append({**result, "scenario": scenario, "run": run_index, "warmup": run_index < settings.warmup})
    return sessions


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--runs", type=int, default=10)
    parser.add_argument("--warmup", type=int, default=1, help="앞에서 버릴 회수(디스크 캐시 워밍업)")
    parser.add_argument("--settle", type=float, default=10.0, help="첫 페인트 뒤 메모리를 재기 전 대기(초)")
    parser.add_argument("--idle", type=float, default=60.0, help="유휴 CPU를 재는 시간(초)")
    parser.add_argument("--keys", type=int, default=10, help="세션마다 넣는 키 수")
    parser.add_argument("--rust-app", type=Path, default=DEFAULT_RUST_APP)
    parser.add_argument("--smoke", action="store_true", help="1회, 짧은 대기. 하네스 동작 확인용이라 수치는 쓰지 않는다")
    parser.add_argument("--warm-cache", action="store_true",
                        help="세션 사이에 XDG_CACHE_HOME·fontconfig 캐시를 이어 쓰고 워밍업 회로 채운 뒤 잰다(기본은 세션마다 새 캐시)")
    parser.add_argument("--force", action="store_true", help="load average가 높아도 본 측정을 한다(수치를 믿을 수 없다)")
    args = parser.parse_args()

    if args.smoke:
        args.runs, args.warmup, args.settle, args.idle, args.keys = 1, 0, 2.0, 5.0, 3
    if args.warm_cache:
        args.warmup = max(args.warmup, 1)
    load_average = os.getloadavg()
    if not args.smoke and not args.force and load_average[0] > MAX_LOAD_AVERAGE:
        print(f"load average {load_average[0]:.2f} > {MAX_LOAD_AVERAGE}: 다른 작업이 끝난 뒤 다시 재거나 --smoke로 확인한다",
              file=sys.stderr)
        return 3
    if not args.rust_app.exists():
        print(f"app binary not found: {args.rust_app}", file=sys.stderr)
        return 2
    started_at = datetime.now(timezone.utc)
    mode = "smoke" if args.smoke else "full"
    cache_suffix = "-warm" if args.warm_cache else ""
    output_dir = OUTPUT_ROOT / f"{started_at.strftime('%Y%m%dT%H%M%SZ')}-{mode}{cache_suffix}"
    output_dir.mkdir(parents=True, exist_ok=True)
    work_root = Path(tempfile.mkdtemp(prefix="kanvibe-perf-"))
    warm_cache_dir = work_root / "warm-cache" if args.warm_cache else None
    settings = Settings(args.runs, args.warmup, args.settle, args.idle, args.keys, args.rust_app.resolve(), warm_cache_dir)
    try:
        sessions = run_all(settings, output_dir, work_root)
    finally:
        shutil.rmtree(work_root, ignore_errors=True)
    commit = subprocess.run(["git", "-C", str(REPO_ROOT), "rev-parse", "--short", "HEAD"], capture_output=True, text=True).stdout.strip()
    report = {
        "meta": {"mode": mode, "cache": "warm" if warm_cache_dir else "cold", "started_at": started_at.isoformat(), "commit": commit, "machine": machine_description(),
                 "load_average_start": [round(value, 2) for value in load_average],
                 "load_average_end": [round(value, 2) for value in os.getloadavg()],
                 "runs": settings.runs, "warmup": settings.warmup, "settle_s": settings.settle_s,
                 "idle_s": settings.idle_s, "key_count": settings.key_count, "rust_app": str(settings.rust_app),
                 "window": list(WINDOW_SIZE), "xvfb": "1600x1000x24 dpi96, no window manager"},
        "summary": summarize_sessions(sessions),
        "sessions": sessions,
    }
    (output_dir / "result.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    write_markdown(report, output_dir / "report.md")
    print(f"[perf] report: {output_dir / 'report.md'}", flush=True)
    return 0 if all(session.get("ok") for session in sessions) else 1


if __name__ == "__main__":
    sys.exit(main())
