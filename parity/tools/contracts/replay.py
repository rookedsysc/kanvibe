#!/usr/bin/env python3
"""TS 앱에서 녹화한 골든 케이스를 서버에 재생해 케이스별 PASS/FAIL을 보고한다.

서버는 빈 데이터베이스로 막 띄운 상태여야 한다. 케이스는 seq 순서로 상태를 쌓아 가므로(페어링 → 프로젝트 → 태스크 → 삭제)
같은 서버에 두 번 재생하면 두 번째는 중복 생성 등으로 실패한다. fixture git 저장소는 재생기가 새로 만든다.

    # 이미 떠 있는 서버
    python3 parity/tools/contracts/replay.py --base-url http://127.0.0.1:39736 --oracle-cmd ./my-oracle
    # 재생기가 같은 격리 환경으로 Rust QA 서버를 새로 띄워 재생
    python3 parity/tools/contracts/replay.py --launch-rust target/debug/kanvibe-server-qa
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from urllib.parse import urlsplit

from kvcontract.fixture import create_fixture
from kvcontract.runner import Endpoints, OracleBackend, Runner
from kvcontract.rustapp import RustApp
from kvcontract.child_environment import REPO_ROOT, MOBILE_TLS_PORT_OFFSET

CONTRACTS_DIR = REPO_ROOT / "parity" / "contracts"


def load_goldens(cases_dir: Path) -> list[dict]:
    manifest = json.loads((cases_dir.parent / "manifest.json").read_text())
    return [json.loads((cases_dir / name).read_text()) for name in manifest["cases"]]


def command_oracle(command: str):
    """`<command> <oracle-name> '<json-args>'`를 실행해 stdout의 JSON을 오라클 값으로 쓴다"""
    def run(name: str, args: dict):
        completed = subprocess.run([*command.split(), name, json.dumps(args)], capture_output=True, text=True, check=True)
        return json.loads(completed.stdout)
    return run


def default_tls_url(base_url: str) -> str:
    parts = urlsplit(base_url)
    return f"https://{parts.hostname}:{parts.port + MOBILE_TLS_PORT_OFFSET}"


def replay(goldens: list[dict], endpoints: Endpoints, oracle: OracleBackend, fixture_root: Path, verbose: bool) -> list:
    variables = create_fixture(fixture_root)
    runner = Runner(endpoints, oracle, variables)
    results = []
    for golden in goldens:
        result = runner.replay(golden)
        results.append(result)
        mark = "PASS" if result.is_passed else "FAIL"
        print(f"{mark} [{golden['seq']:03d}] {golden['id']}", flush=True)
        if not result.is_passed:
            for error in result.errors[:8]:
                print(f"      {error[:400]}")
            if verbose:
                print("      observed: " + json.dumps(result.observed, ensure_ascii=False)[:1500])
    return results


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    target = parser.add_mutually_exclusive_group(required=True)
    target.add_argument("--base-url", help="plaintext hook listener, e.g. http://127.0.0.1:39736")
    target.add_argument("--launch-rust", type=Path, metavar="BINARY", help="launch a fresh isolated `kanvibe-server-qa` and replay against it")
    parser.add_argument("--tls-url", help="mobile TLS listener (default: base-url port + 10, https)")
    parser.add_argument("--oracle-cmd", help="command answering server oracles: <cmd> <name> <json-args> -> JSON on stdout")
    parser.add_argument("--hook-port", type=int, default=39936, help="hook port for --launch-rust")
    parser.add_argument("--cases", type=Path, default=CONTRACTS_DIR / "cases")
    parser.add_argument("--fixture-root", type=Path, help="directory for the fixture git repositories (default: new temp dir)")
    parser.add_argument("--report", type=Path, help="write a JSON report here")
    parser.add_argument("--verbose", action="store_true")
    arguments = parser.parse_args()

    goldens = load_goldens(arguments.cases)
    if arguments.fixture_root and arguments.fixture_root.exists() and any(arguments.fixture_root.iterdir()):
        parser.error(f"--fixture-root {arguments.fixture_root} must be empty")
    launches_app = arguments.launch_rust is not None
    owns_fixture = arguments.fixture_root is None and not launches_app
    fixture_root = arguments.fixture_root or (None if launches_app else Path(tempfile.mkdtemp(prefix="kanvibe-contract-fixture-")))
    app = None
    try:
        if arguments.launch_rust:
            app = RustApp(arguments.launch_rust, arguments.hook_port)
            app.start()
            endpoints = Endpoints(app.plain_url, app.tls_url)
            oracle = OracleBackend(app.run_oracle)
            fixture_root = app.run_dir / "fixture"
        else:
            endpoints = Endpoints(arguments.base_url, arguments.tls_url or default_tls_url(arguments.base_url))
            oracle = OracleBackend(command_oracle(arguments.oracle_cmd) if arguments.oracle_cmd else None)
        results = replay(goldens, endpoints, oracle, fixture_root, arguments.verbose)
    finally:
        if app:
            app.stop()
        elif owns_fixture:
            shutil.rmtree(fixture_root, ignore_errors=True)

    passed = sum(result.is_passed for result in results)
    print(f"\n{passed}/{len(results)} cases passed")
    if arguments.report:
        arguments.report.write_text(json.dumps({
            "passed": passed, "total": len(results),
            "failures": [{"id": result.case_id, "errors": result.errors} for result in results if not result.is_passed],
        }, ensure_ascii=False, indent=2) + "\n")
    return 0 if passed == len(results) else 1


if __name__ == "__main__":
    sys.exit(main())
