#!/usr/bin/env python3
"""스크린샷 기준 이미지용 로컬 git 저장소와 worktree를 만든다.

네트워크 clone이나 원격 아이콘 없이, 고정된 작성자와 고정된 커밋 시각으로 매번 같은 커밋 해시가 나오게 만든다.
원격(origin)은 두지 않는다. 앱이 GitHub 아이콘이나 PR을 원격에서 찾지 않게 하기 위해서다.

사용법: python3 make_git_repos.py <fixture-root>
결과: <fixture-root>/repos/<repo> 와 <fixture-root>/repos/<repo>__worktrees/<branch-with-dashes>
"""
from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

GIT_IDENTITY = {
    "GIT_AUTHOR_NAME": "Parity Fixture",
    "GIT_AUTHOR_EMAIL": "parity@example.invalid",
    "GIT_COMMITTER_NAME": "Parity Fixture",
    "GIT_COMMITTER_EMAIL": "parity@example.invalid",
}

# 커밋마다 이 시각부터 1시간씩 늘린다. FIXED_NOW(2026-01-15T03:00:00Z)보다 앞선다.
BASE_COMMIT_EPOCH = 1768262400  # 2026-01-13T00:00:00Z


def build_git_environment(home_dir: Path) -> dict[str, str]:
    """사용자 git 설정(서명, hook, 기본 브랜치)이 결과에 섞이지 않게 git 전용 환경을 만든다."""
    return {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": str(home_dir),
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_GLOBAL": "/dev/null",
        "LANG": "C.UTF-8",
        "TZ": "UTC",
        **GIT_IDENTITY,
    }


class FixtureRepository:
    def __init__(self, path: Path, git_environment: dict[str, str]):
        self.path = path
        self.git_environment = git_environment
        self.commit_count = 0

    def run_git(self, *args: str, cwd: Path | None = None) -> str:
        completed = subprocess.run(
            ["git", *args],
            cwd=cwd or self.path,
            env=self.git_environment,
            check=True,
            capture_output=True,
            text=True,
        )
        return completed.stdout.strip()

    def write_files(self, files: dict[str, str], cwd: Path | None = None) -> None:
        base = cwd or self.path
        for relative_path, content in files.items():
            file_path = base / relative_path
            file_path.parent.mkdir(parents=True, exist_ok=True)
            file_path.write_text(content, encoding="utf-8")

    def commit_all(self, message: str, cwd: Path | None = None) -> None:
        commit_epoch = BASE_COMMIT_EPOCH + self.commit_count * 3600
        self.commit_count += 1
        timestamp = f"@{commit_epoch} +0000"
        self.git_environment["GIT_AUTHOR_DATE"] = timestamp
        self.git_environment["GIT_COMMITTER_DATE"] = timestamp
        self.run_git("add", "-A", cwd=cwd)
        self.run_git("commit", "-q", "--no-gpg-sign", "-m", message, cwd=cwd)

    def add_worktree(self, branch_name: str, base_branch: str = "main") -> Path:
        worktree_path = self.path.parent / f"{self.path.name}__worktrees" / branch_name.replace("/", "-")
        worktree_path.parent.mkdir(parents=True, exist_ok=True)
        self.run_git("worktree", "add", "-q", "-b", branch_name, str(worktree_path), base_branch)
        return worktree_path


def init_repository(path: Path, git_environment: dict[str, str]) -> FixtureRepository:
    path.mkdir(parents=True, exist_ok=True)
    repository = FixtureRepository(path, dict(git_environment))
    repository.run_git("init", "-q", "-b", "main")
    repository.run_git("config", "core.autocrlf", "false")
    repository.run_git("config", "commit.gpgsign", "false")
    return repository


KANVIBE_DEMO_BASE_FILES = {
    "README.md": "# kanvibe-demo\n\nA small fixture project for KanVibe screenshot parity.\n",
    "package.json": '{\n  "name": "kanvibe-demo",\n  "version": "0.1.0",\n  "private": true\n}\n',
    "src/board.ts": (
        "export interface Card {\n"
        "  id: string;\n"
        "  title: string;\n"
        "  status: \"todo\" | \"progress\" | \"done\";\n"
        "}\n\n"
        "export function sortCards(cards: Card[]): Card[] {\n"
        "  return [...cards].sort((left, right) => left.title.localeCompare(right.title));\n"
        "}\n\n"
        "export function countDone(cards: Card[]): number {\n"
        "  return cards.filter((card) => card.status === \"done\").length;\n"
        "}\n"
    ),
    "src/app.ts": (
        "import { sortCards } from \"./board\";\n\n"
        "export function main(): void {\n"
        "  const cards = sortCards([]);\n"
        "  console.log(`cards: ${cards.length}`);\n"
        "}\n"
    ),
    "src/legacy.ts": "// 곧 지울 예정인 옛 진입점\nexport const LEGACY_FLAG = true;\n",
    "docs/guide.md": "# Guide\n\n- Create a task\n- Move it across the board\n",
}


def build_kanvibe_demo(repos_root: Path, git_environment: dict[str, str]) -> None:
    repository = init_repository(repos_root / "kanvibe-demo", git_environment)
    repository.write_files(KANVIBE_DEMO_BASE_FILES)
    repository.commit_all("chore: initial import")
    repository.write_files({"docs/guide.md": KANVIBE_DEMO_BASE_FILES["docs/guide.md"] + "- Review the diff\n"})
    repository.commit_all("docs: describe the review step")

    # 진행 중 태스크: 커밋된 변경 + 아직 커밋하지 않은 변경을 함께 둬 diff 화면에 수정·추가·삭제가 모두 보이게 한다
    board_filters = repository.add_worktree("feat/board-filters")
    repository.write_files({"docs/filters.md": "# Filters\n\nProject and status filters live in the board header.\n"}, cwd=board_filters)
    repository.commit_all("docs: describe board filters", cwd=board_filters)
    repository.write_files({
        "src/filters.ts": (
            "import type { Card } from \"./board\";\n\n"
            "export type CardFilter = (card: Card) => boolean;\n\n"
            "export const onlyOpen: CardFilter = (card) => card.status !== \"done\";\n\n"
            "export function applyFilters(cards: Card[], filters: CardFilter[]): Card[] {\n"
            "  return cards.filter((card) => filters.every((filter) => filter(card)));\n"
            "}\n"
        ),
    }, cwd=board_filters)
    (board_filters / "src/legacy.ts").unlink()
    repository.write_files({
        "src/board.ts": KANVIBE_DEMO_BASE_FILES["src/board.ts"].replace(
            "export function countDone",
            "export function countOpen(cards: Card[]): number {\n"
            "  return cards.length - countDone(cards);\n"
            "}\n\n"
            "export function countDone",
        ).replace("left.title.localeCompare(right.title)", "left.title.localeCompare(right.title, \"ko\")"),
    }, cwd=board_filters)

    login_redirect = repository.add_worktree("fix/login-redirect")
    repository.write_files({
        "src/app.ts": KANVIBE_DEMO_BASE_FILES["src/app.ts"].replace("cards: ${cards.length}", "redirect after login: ${cards.length}"),
    }, cwd=login_redirect)
    repository.commit_all("fix: keep the redirect target after login", cwd=login_redirect)
    repository.write_files({"README.md": KANVIBE_DEMO_BASE_FILES["README.md"] + "\nLogin keeps the redirect target.\n"}, cwd=login_redirect)

    # 변경이 없는 worktree: diff 화면의 "변경 없음" 상태에 쓴다
    repository.add_worktree("feat/dark-mode")


def build_api_server(repos_root: Path, git_environment: dict[str, str]) -> None:
    repository = init_repository(repos_root / "api-server", git_environment)
    repository.write_files({
        "README.md": "# api-server\n",
        "server/main.py": "def handler(request):\n    return {\"ok\": True}\n",
        "server/limits.py": "MAX_REQUESTS_PER_MINUTE = 60\n",
    })
    repository.commit_all("chore: initial import")
    rate_limit = repository.add_worktree("feat/rate-limit")
    repository.write_files({
        "server/limits.py": "MAX_REQUESTS_PER_MINUTE = 120\nBURST = 20\n",
        "server/middleware.py": "from server.limits import BURST\n\n\ndef allow(count):\n    return count <= BURST\n",
    }, cwd=rate_limit)
    repository.commit_all("feat: add burst limit", cwd=rate_limit)
    repository.write_files({
        "server/limits.py": "MAX_REQUESTS_PER_MINUTE = 120\nBURST = 30\nWINDOW_SECONDS = 60\n",
    }, cwd=rate_limit)


def build_mobile_app(repos_root: Path, git_environment: dict[str, str]) -> None:
    repository = init_repository(repos_root / "mobile-app", git_environment)
    repository.write_files({
        "README.md": "# mobile-app\n",
        "lib/main.dart": "void main() {\n  print('hello');\n}\n",
    })
    repository.commit_all("chore: initial import")
    repository.add_worktree("feat/push-settings")


def build_all_repositories(fixture_root: Path, home_dir: Path) -> Path:
    repos_root = fixture_root / "repos"
    if repos_root.exists():
        shutil.rmtree(repos_root)
    repos_root.mkdir(parents=True)
    git_environment = build_git_environment(home_dir)
    build_kanvibe_demo(repos_root, git_environment)
    build_api_server(repos_root, git_environment)
    build_mobile_app(repos_root, git_environment)
    return repos_root


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    root = Path(sys.argv[1]).resolve()
    (root / "home").mkdir(parents=True, exist_ok=True)
    print(build_all_repositories(root, root / "home"))
