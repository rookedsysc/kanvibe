#!/usr/bin/env python3
"""TS(Electron) 앱의 HTTP/WebSocket 동작을 골든 파일로 녹화한다.

빌드된 앱(build/)을 격리 디렉터리에서 xvfb-run으로 띄우고, scenario.py의 요청을 순서대로 보내 받은 응답을
변동값 규칙으로 정규화해 parity/contracts/cases/에 쓴다. 네이티브 모듈 재빌드나 pnpm 명령은 실행하지 않는다.

    python3 parity/tools/contracts/record.py [--hook-port 39836] [--inspector-port 39329]
"""

from __future__ import annotations

import argparse
import json
import shutil
import sys
from pathlib import Path

from kvcontract.fixture import create_fixture
from kvcontract.runner import Endpoints, OracleBackend, Runner
from kvcontract.tsapp import REPO_ROOT, TsApp
from scenario import build_cases
import coverage

CONTRACTS_DIR = REPO_ROOT / "parity" / "contracts"
FORMAT_VERSION = 1


def case_file_name(seq: int, case_id: str) -> str:
    return f"{seq:03d}_{case_id}.json"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--hook-port", type=int, default=39836)
    parser.add_argument("--inspector-port", type=int, default=39329)
    parser.add_argument("--out", type=Path, default=CONTRACTS_DIR / "cases")
    parser.add_argument("--keep-run-dir", action="store_true", help="keep the isolated app/fixture directory for debugging")
    arguments = parser.parse_args()

    cases = build_cases()
    app = TsApp(arguments.hook_port, arguments.inspector_port, keep_run_dir=arguments.keep_run_dir)
    goldens = []
    try:
        app.start()
        variables = create_fixture(app.run_dir / "fixture")
        runner = Runner(Endpoints(app.plain_url, app.tls_url), OracleBackend(app.run_oracle), variables)
        for seq, case in enumerate(cases, start=1):
            golden = runner.record(case, substitutable=list(variables))
            golden = {"seq": seq, **golden}
            goldens.append(golden)
            expect = golden["expect"]
            status = expect.get("status", expect.get("outcome", expect.get("handshake", {}).get("status", "ok")))
            print(f"[{seq:03d}] {case['id']}: {status}", flush=True)
    finally:
        app.stop()
        if arguments.keep_run_dir:
            print(f"run dir kept: {app.run_dir}")

    if arguments.out.exists():
        shutil.rmtree(arguments.out)
    arguments.out.mkdir(parents=True)
    for golden in goldens:
        path = arguments.out / case_file_name(golden["seq"], golden["id"])
        path.write_text(json.dumps(golden, ensure_ascii=False, indent=2) + "\n")
    manifest = {
        "format_version": FORMAT_VERSION,
        "source": "TS Electron app (build/) recorded by parity/tools/contracts/record.py",
        "case_count": len(goldens),
        "cases": [case_file_name(golden["seq"], golden["id"]) for golden in goldens],
    }
    (arguments.out.parent / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
    coverage.write_coverage(goldens, arguments.out.parent / "coverage.md")
    print(f"wrote {len(goldens)} golden cases to {arguments.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
