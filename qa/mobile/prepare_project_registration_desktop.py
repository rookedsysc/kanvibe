#!/usr/bin/env python3
"""모바일 프로젝트 등록 흐름(project-registration)이 상대할 Rust 데스크톱.

hook 서버 20736(모바일 TLS 20746)과 등록 probe 20737을 고정으로 쓴다. 등록할 git 저장소 하나를 `--run-dir/repositories`에
만들고, 데스크톱의 보드는 비워 둔다. probe `GET /state`는 관측용 기기가 모바일 API로 읽은 실제 보드를 돌려준다.

예:
  python3 qa/mobile/prepare_project_registration_desktop.py --run-dir qa-results/mobile/registration-1
"""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from desktop_fixture import (  # noqa: E402
    IsolatedDesktop, MobileObserver, ProbeServer, base_argument_parser, create_repository, require_fresh_run_dir,
    run_fixture,
)

DESKTOP_HOOK_PORT = 20736
PROBE_PORT = 20737
PROJECT_NAME = "maestro-mobile-register"


def prepare(desktop: IsolatedDesktop):
    run_dir = desktop.run_dir
    repo_path = create_repository(run_dir / "repositories" / PROJECT_NAME, "# Mobile registration fixture\n",
                                  desktop.environment)
    desktop.start()
    # 설정은 호출마다 DB에서 읽으므로 띄운 채 바꿔도 된다. 등록이 만드는 루트 태스크가 tmux·herdr 세션을 띄우지 않게 한다
    desktop.save_app_setting("default_session_type", "terminal")
    desktop.save_app_setting("background_sync_enabled", "false")
    observer = MobileObserver(desktop, "Registration QA observer")
    fixture = {
        "repoPath": str(repo_path),
        "browsePath": str(repo_path.parent),
        "projectName": PROJECT_NAME,
        "hookPort": desktop.hook_port,
        "mobilePort": desktop.mobile_tls_port,
        "appDataDir": str(desktop.data_dir),
    }

    def state(_method: str, _query: dict[str, str]) -> dict:
        return {"fixture": fixture, "board": observer.board()}

    probe = ProbeServer(PROBE_PORT, {"/state": state}, run_dir / "observations.jsonl")
    desktop.screenshot("desktop-fixture")
    return fixture, probe.close


def main() -> None:
    arguments = base_argument_parser(__doc__).parse_args()
    run_dir = require_fresh_run_dir(arguments.run_dir)
    run_fixture(prepare, run_dir, Path(arguments.app), DESKTOP_HOOK_PORT, [PROBE_PORT])


if __name__ == "__main__":
    main()
