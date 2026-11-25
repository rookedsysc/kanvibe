"""골든이 기대하는 git 저장소 fixture를 만든다. 녹화와 재생이 같은 함수를 써서 같은 내용을 얻는다.

커밋 해시가 실행마다 같도록 작성자·시각을 고정한다. 사용자의 전역 git 설정은 읽지 않는다.
"""

from __future__ import annotations

import os
import subprocess
from pathlib import Path

FIXED_DATE = "2026-01-01T00:00:00+00:00"


def run_fixture_git(repository: Path, *arguments: str) -> None:
    environment = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": str(repository),
        "GIT_CONFIG_GLOBAL": "/dev/null",
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_AUTHOR_NAME": "KanVibe Contract",
        "GIT_AUTHOR_EMAIL": "contract@kanvibe.invalid",
        "GIT_COMMITTER_NAME": "KanVibe Contract",
        "GIT_COMMITTER_EMAIL": "contract@kanvibe.invalid",
        "GIT_AUTHOR_DATE": FIXED_DATE,
        "GIT_COMMITTER_DATE": FIXED_DATE,
    }
    subprocess.run(["git", *arguments], cwd=repository, env=environment, check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)


def _create_repository(path: Path, files: dict[str, str], extra_branches: dict[str, dict[str, str]]) -> None:
    path.mkdir(parents=True)
    run_fixture_git(path, "init", "-q", "-b", "main")
    for relative, content in files.items():
        target = path / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content)
    run_fixture_git(path, "add", "-A")
    run_fixture_git(path, "commit", "-q", "-m", "initial")
    for branch, branch_files in extra_branches.items():
        run_fixture_git(path, "branch", branch)
        if branch_files:
            run_fixture_git(path, "checkout", "-q", branch)
            for relative, content in branch_files.items():
                (path / relative).write_text(content)
            run_fixture_git(path, "add", "-A")
            run_fixture_git(path, "commit", "-q", "-m", f"{branch} change")
            run_fixture_git(path, "checkout", "-q", "main")


def create_fixture(fixture_root: Path) -> dict[str, str]:
    """fixture를 만들고 케이스가 참조할 경로 변수를 돌려준다"""
    # 스캔 한 번에 저장소가 둘이면 등록 순서가 디렉터리 읽기 순서를 따라 흔들리므로, 스캔 루트마다 저장소를 하나만 둔다
    repositories = fixture_root / "repos"
    _create_repository(
        repositories / "demo-group" / "demo",
        {"README.md": "# demo\n", "notes/todo.txt": "first line\nsecond line\n"},
        {"existing-branch": {"notes/todo.txt": "first line\nchanged on branch\n"}},
    )
    _create_repository(repositories / "scratch-group" / "scratch", {"README.md": "# scratch\n"}, {})
    return {
        "fixture_root": str(fixture_root),
        "demo_scan_root": str(repositories / "demo-group"),
        "scratch_scan_root": str(repositories / "scratch-group"),
        "demo_repo": str(repositories / "demo-group" / "demo"),
        "scratch_repo": str(repositories / "scratch-group" / "scratch"),
    }
