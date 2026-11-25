#!/usr/bin/env python3
"""스크린샷 기준 이미지용 결정적 fixture를 만든다. 특정 앱(Electron/GPUI)에 묶이지 않는다.

한 번 실행하면 <root> 아래에 다음을 새로 만든다(기존 내용은 지운다).
  repos/      로컬 git 저장소와 worktree (make_git_repos.py)
  home/       앱 프로세스에만 넘기는 격리 HOME (make_home.py)
  bin/        PATH 앞에 두는 가짜 CLI(gh, code, claude, codex) (make_home.py)
  appdata/kanvibe.db   schema.sql(TS 앱이 만든 스키마) + 데이터셋 행

경로가 화면에 그대로 찍히므로 <root>는 실행마다 같은 절대 경로여야 한다.

사용법:
  python3 build_fixture.py --root /tmp/kanvibe-parity/fixture --dataset full \
      --set theme_preference=light --set board_view_mode=workspace
"""
from __future__ import annotations

import argparse
import json
import shutil
import sqlite3
import sys
from pathlib import Path

FIXTURES_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(FIXTURES_DIR))

from make_git_repos import build_all_repositories  # noqa: E402
from make_home import build_home, solid_png_data_url  # noqa: E402

FIXED_NOW_ISO = "2026-01-15T03:00:00Z"
FIXED_NOW_EPOCH_MS = 1768446000000


def at(minutes_before_now: int) -> str:
    """FIXED_NOW에서 N분 전 시각을 TypeORM이 쓰는 'YYYY-MM-DD HH:MM:SS'(UTC) 문자열로 만든다."""
    epoch_seconds = FIXED_NOW_EPOCH_MS // 1000 - minutes_before_now * 60
    import datetime as _dt

    return _dt.datetime.fromtimestamp(epoch_seconds, tz=_dt.timezone.utc).strftime("%Y-%m-%d %H:%M:%S")


def iso_at(minutes_before_now: int) -> str:
    import datetime as _dt

    epoch_seconds = FIXED_NOW_EPOCH_MS // 1000 - minutes_before_now * 60
    return _dt.datetime.fromtimestamp(epoch_seconds, tz=_dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.000Z")


PROJECT_IDS = {
    "kanvibe-demo": "11111111-1111-4111-8111-000000000001",
    "api-server": "11111111-1111-4111-8111-000000000002",
    "mobile-app": "11111111-1111-4111-8111-000000000003",
    "infra": "11111111-1111-4111-8111-000000000004",
}

TASK_IDS = {
    "plan-filters": "22222222-2222-4222-8222-000000000001",
    "onboarding-doc": "22222222-2222-4222-8222-000000000002",
    "demo-base": "22222222-2222-4222-8222-000000000003",
    "board-filters": "22222222-2222-4222-8222-000000000010",
    "rate-limit": "22222222-2222-4222-8222-000000000011",
    "push-settings": "22222222-2222-4222-8222-000000000012",
    "dark-mode": "22222222-2222-4222-8222-000000000020",
    "login-redirect": "22222222-2222-4222-8222-000000000030",
    "deploy-script": "22222222-2222-4222-8222-000000000031",
    "release-notes": "22222222-2222-4222-8222-000000000040",
    "ci-cache": "22222222-2222-4222-8222-000000000041",
    "readme-badges": "22222222-2222-4222-8222-000000000042",
}

# 16x16 단색 PNG. 원격 아이콘 대신 쓰는 고정 프로젝트 아이콘이다.
MOBILE_APP_ICON_DATA_URL = solid_png_data_url(16, 16, (240, 140, 40))


def build_projects(root: Path) -> list[dict]:
    repos = root / "repos"
    return [
        {"id": PROJECT_IDS["kanvibe-demo"], "name": "kanvibe-demo", "repo_path": str(repos / "kanvibe-demo"),
         "default_branch": "main", "ssh_host": None, "is_worktree": 0, "color": "#7c5cff", "icon_data_url": None,
         "created_at": at(60 * 24 * 7)},
        {"id": PROJECT_IDS["api-server"], "name": "api-server", "repo_path": str(repos / "api-server"),
         "default_branch": "main", "ssh_host": None, "is_worktree": 0, "color": "#22a06b", "icon_data_url": None,
         "created_at": at(60 * 24 * 6)},
        {"id": PROJECT_IDS["mobile-app"], "name": "mobile-app", "repo_path": str(repos / "mobile-app"),
         "default_branch": "main", "ssh_host": None, "is_worktree": 0, "color": None,
         "icon_data_url": MOBILE_APP_ICON_DATA_URL, "created_at": at(60 * 24 * 5)},
        {"id": PROJECT_IDS["infra"], "name": "infra", "repo_path": "/srv/infra",
         "default_branch": "main", "ssh_host": "devbox", "is_worktree": 0, "color": "#e5484d", "icon_data_url": None,
         "created_at": at(60 * 24 * 4)},
    ]


def worktree(root: Path, repo: str, branch: str) -> str:
    return str(root / "repos" / f"{repo}__worktrees" / branch.replace("/", "-"))


def task_row(key: str, title: str, status: str, minutes_ago: int, **fields) -> dict:
    row = {
        "id": TASK_IDS.get(key, key),
        "title": title,
        "description": None,
        "status": status,
        "branch_name": None,
        "worktree_path": None,
        "session_type": None,
        "session_name": None,
        "ssh_host": None,
        "agent_type": None,
        "project_id": None,
        "base_branch": None,
        "pr_url": None,
        "priority": None,
        "created_at": at(minutes_ago + 60 * 24),
        "updated_at": at(minutes_ago),
    }
    row.update(fields)
    return row


def build_tasks(root: Path, extra_done_count: int) -> list[dict]:
    demo = PROJECT_IDS["kanvibe-demo"]
    tasks = [
        task_row("plan-filters", "보드 필터 저장 위치 정하기", "todo", 30, project_id=demo, priority="medium",
                 description="필터 상태를 URL에 둘지 앱 설정에 둘지 정한다."),
        task_row("onboarding-doc", "온보딩 문서 초안", "todo", 95, priority="low",
                 description="새 팀원이 첫날 따라 할 설치 순서를 적는다."),
        task_row("demo-base", "kanvibe-demo", "todo", 240, project_id=demo, branch_name="main", base_branch="main",
                 worktree_path=str(root / "repos" / "kanvibe-demo"), session_type="tmux", session_name="kanvibe-demo-main"),
        task_row("board-filters", "보드 필터 추가", "progress", 5, project_id=demo, branch_name="feat/board-filters",
                 base_branch="main", worktree_path=worktree(root, "kanvibe-demo", "feat/board-filters"),
                 session_type="tmux", session_name="kanvibe-demo-feat-board-filters", agent_type="claude",
                 priority="high", pr_url="https://github.com/example/kanvibe-demo/pull/42",
                 description="프로젝트·상태별 필터를 보드 헤더에 추가하고 선택을 기억한다."),
        task_row("rate-limit", "요청 속도 제한", "progress", 12, project_id=PROJECT_IDS["api-server"],
                 branch_name="feat/rate-limit", base_branch="main",
                 worktree_path=worktree(root, "api-server", "feat/rate-limit"), session_type="zellij",
                 session_name="api-server-feat-rate-limit", agent_type="opencode", priority="medium"),
        task_row("push-settings", "푸시 알림 설정 화면", "progress", 48, project_id=PROJECT_IDS["mobile-app"],
                 branch_name="feat/push-settings", base_branch="main",
                 worktree_path=worktree(root, "mobile-app", "feat/push-settings"), session_type="terminal",
                 session_name="mobile-app-feat-push-settings"),
        task_row("dark-mode", "다크 모드 지원", "pending", 20, project_id=demo, branch_name="feat/dark-mode",
                 base_branch="main", worktree_path=worktree(root, "kanvibe-demo", "feat/dark-mode"),
                 session_type="herdr", session_name="kanvibe-demo-feat-dark-mode", agent_type="gemini", priority="low",
                 description="시스템 테마를 따라가도록 색 토큰을 나눈다."),
        task_row("login-redirect", "로그인 후 리다이렉트 수정", "review", 15, project_id=demo,
                 branch_name="fix/login-redirect", base_branch="main",
                 worktree_path=worktree(root, "kanvibe-demo", "fix/login-redirect"), session_type="tmux",
                 session_name="kanvibe-demo-fix-login-redirect", agent_type="codex", priority="medium",
                 pr_url="https://github.com/example/kanvibe-demo/pull/41"),
        task_row("deploy-script", "배포 스크립트 정리", "review", 70, project_id=PROJECT_IDS["infra"],
                 branch_name="chore/deploy", base_branch="main", worktree_path="/srv/infra__worktrees/chore-deploy",
                 session_type="tmux", session_name="infra-chore-deploy", ssh_host="devbox"),
        task_row("release-notes", "1.5 릴리스 노트 작성", "done", 60 * 24, project_id=demo),
        task_row("ci-cache", "CI 캐시 키 정리", "done", 60 * 26, project_id=PROJECT_IDS["api-server"], priority="low"),
        task_row("readme-badges", "README 배지 추가", "done", 60 * 30, project_id=demo),
    ]
    for index in range(extra_done_count):
        tasks.append(task_row(
            f"33333333-3333-4333-8333-{index:012d}", f"완료된 정리 작업 {index + 1:02d}", "done",
            60 * 31 + index * 10, project_id=demo,
        ))
    return tasks


def build_pane_layouts() -> list[dict]:
    return [
        {"id": "44444444-4444-4444-8444-000000000001", "layout_type": "single",
         "panes": json.dumps([{"position": 0, "command": ""}]), "project_id": None, "is_global": 1,
         "created_at": at(60 * 24 * 7), "updated_at": at(60 * 24 * 7)},
        {"id": "44444444-4444-4444-8444-000000000002", "layout_type": "vertical_2",
         "panes": json.dumps([{"position": 0, "command": "python3 -m http.server"}, {"position": 1, "command": "git status"}]),
         "project_id": PROJECT_IDS["api-server"], "is_global": 0,
         "created_at": at(60 * 24 * 6), "updated_at": at(60 * 24 * 6)},
    ]


def build_notifications() -> list[dict]:
    return [
        {"id": "55555555-5555-4555-8555-000000000001", "title": "보드 필터 추가",
         "body": "Review 상태로 바뀌었습니다", "taskId": TASK_IDS["board-filters"],
         "relativePath": f"/ko/task/{TASK_IDS['board-filters']}", "locale": "ko", "isRead": False,
         "createdAt": iso_at(3), "dedupeKey": "parity-1"},
        {"id": "55555555-5555-4555-8555-000000000002", "title": "로그인 후 리다이렉트 수정",
         "body": "입력을 기다리고 있습니다", "taskId": TASK_IDS["login-redirect"],
         "relativePath": f"/ko/task/{TASK_IDS['login-redirect']}", "locale": "ko", "isRead": False,
         "createdAt": iso_at(9), "dedupeKey": "parity-2"},
        {"id": "55555555-5555-4555-8555-000000000003", "title": "요청 속도 제한",
         "body": "Progress 상태로 바뀌었습니다", "taskId": TASK_IDS["rate-limit"],
         "relativePath": f"/ko/task/{TASK_IDS['rate-limit']}", "locale": "ko", "isRead": True,
         "createdAt": iso_at(40), "dedupeKey": "parity-3"},
    ]


# 앱이 스스로 계산해 저장하는 값과 같게 둔다. 다르면 첫 화면과 갱신 뒤 화면이 달라져 결정성이 깨진다.
DIFF_STATS = {
    "board-filters": (3, 14, 3),
    "login-redirect": (1, 2, 0),
    "rate-limit": (1, 2, 1),
    "push-settings": (0, 0, 0),
}


def build_settings(root: Path, overrides: dict[str, str], extra_notifications: bool) -> dict[str, str]:
    settings = {
        "hook_client_id": "66666666-6666-4666-8666-000000000001",
        "lan_plaintext_observed_since": str(FIXED_NOW_EPOCH_MS - 86_400_000),
        "task_cli_secret": "parity-fixture-task-cli-secret-0000000000000000",
        "background_sync_enabled": "false",
        "theme_preference": "dark",
        "notification_enabled": "true",
        "app_notifications": json.dumps(build_notifications() if extra_notifications else [], ensure_ascii=False),
        "ai_accounts": json.dumps([]),
    }
    settings.update(overrides)
    return {key: value for key, value in settings.items() if value != "__unset__"}


DATASETS = {
    # 이름: (프로젝트 포함, 태스크 포함, 추가 done 수, 알림 포함)
    "full": (True, True, 0, True),
    "no-sessions": (True, True, 0, False),
    "many-done": (True, True, 22, True),
    "projects-only": (True, False, 0, False),
    "empty": (False, False, 0, False),
}


def insert_rows(connection: sqlite3.Connection, table: str, rows: list[dict]) -> None:
    for row in rows:
        columns = ", ".join(f'"{column}"' for column in row)
        placeholders = ", ".join("?" for _ in row)
        connection.execute(f'INSERT INTO "{table}" ({columns}) VALUES ({placeholders})', list(row.values()))


def build_database(root: Path, dataset: str, overrides: dict[str, str]) -> Path:
    include_projects, include_tasks, extra_done_count, include_notifications = DATASETS[dataset]
    database_path = root / "appdata" / "kanvibe.db"
    database_path.parent.mkdir(parents=True, exist_ok=True)
    connection = sqlite3.connect(database_path)
    connection.executescript((FIXTURES_DIR / "schema.sql").read_text(encoding="utf-8"))

    if include_projects:
        insert_rows(connection, "projects", build_projects(root))
        insert_rows(connection, "pane_layout_configs", build_pane_layouts())
    if include_tasks:
        tasks = build_tasks(root, extra_done_count)
        if dataset == "no-sessions":
            # 워크스페이스의 "선택 없음" 상태용: 어떤 태스크도 터미널 세션을 갖지 않는다
            for task in tasks:
                task.update({"session_type": None, "session_name": None, "worktree_path": None})
        insert_rows(connection, "kanban_tasks", tasks)
        insert_rows(connection, "task_diff_stats", [
            {"task_id": TASK_IDS[key], "file_count": files, "additions": additions, "deletions": deletions,
             "updated_at": at(1)}
            for key, (files, additions, deletions) in DIFF_STATS.items()
        ])

    settings = build_settings(root, overrides, include_notifications)
    insert_rows(connection, "app_settings", [
        {"id": f"77777777-7777-4777-8777-{index:012d}", "key": key, "value": value,
         "created_at": at(60 * 24 * 7), "updated_at": at(60 * 24 * 7)}
        for index, (key, value) in enumerate(sorted(settings.items()))
    ])
    connection.commit()
    connection.close()
    return database_path


def parse_overrides(pairs: list[str]) -> dict[str, str]:
    overrides = {}
    for pair in pairs:
        key, separator, value = pair.partition("=")
        if not separator:
            raise SystemExit(f"--set needs key=value: {pair}")
        overrides[key] = value
    return overrides


def build_fixture(root: Path, dataset: str, overrides: dict[str, str], extras: list[str] | None = None,
                  home_dir: Path | None = None) -> dict[str, str]:
    """root·home_dir를 명시적으로 받는다. home_dir 기본값은 <root>/home이며 실제 사용자 HOME은 쓰지 않는다."""
    home_dir = (home_dir or root / "home").resolve()
    if not home_dir.is_relative_to(root.resolve()):
        raise SystemExit(f"fixture HOME must live under the fixture root: {home_dir}")
    if root.exists():
        shutil.rmtree(root)
    root.mkdir(parents=True)
    build_home(root, home_dir, FIXED_NOW_EPOCH_MS, extras or [])
    build_all_repositories(root, home_dir)
    database_path = build_database(root, dataset, overrides)
    return {"root": str(root), "home": str(home_dir), "bin": str(root / "bin"),
            "appdata": str(database_path.parent), "database": str(database_path)}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--root", required=True)
    parser.add_argument("--home", help="격리 HOME 경로(기본 <root>/home). 실제 HOME을 가리키면 거부한다")
    parser.add_argument("--dataset", default="full", choices=sorted(DATASETS))
    parser.add_argument("--set", action="append", default=[], help="app_settings key=value (value __unset__ removes)")
    parser.add_argument("--extra", action="append", default=[], help="HOME에 더할 fixture 묶음 이름(make_home.EXTRAS)")
    args = parser.parse_args()
    root = Path(args.root).resolve()
    result = build_fixture(root, args.dataset, parse_overrides(args.set), args.extra,
                           Path(args.home).resolve() if args.home else None)
    print(json.dumps(result))


if __name__ == "__main__":
    main()
