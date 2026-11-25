#!/usr/bin/env python3
"""모바일 프로젝트 필터·알림 흐름(project-filter-notifications, task-detail-notification-read)이 상대할 Rust 데스크톱.

hook 서버 39736(모바일 TLS 39746)과 알림 probe 19737을 고정으로 쓴다. 프로젝트 둘(Alpha, Beta)과 worktree 태스크, 알림 넷을
격리 DB에 둔다. 프로젝트 등록과 worktree 태스크 생성은 관측용 기기로 실제 모바일 API(`/api/mobile/projects/scan`,
`/api/mobile/tasks`)를 거쳐 만들고, 이름과 화면 밖 상태(알림 목록, 브랜치 없는 태스크)는 데스크톱을 끈 사이에 DB에 넣는다.

probe(`GET /state`, `POST /desktop/<동작>`)는 TS fixture와 같은 응답 모양을 쓴다.
  - 알림·보드는 관측용 기기가 모바일 API로 읽은 실제 저장 상태다.
  - `desktopEventCount`는 데스크톱 알림 패널(안읽음만 보기로 열어 둔 채)이 새 저장 상태를 그려 보인 횟수다.
    패널을 다시 열지 않고 확인하므로, 데스크톱이 변경 신호를 받아 다시 그렸을 때만 오른다.
  - read-one·read-all은 데스크톱 패널을 QA 제어 소켓으로 눌러 한다. create·create-task-notification은 보드 이벤트
    (`task-status-changed`)를 데스크톱 런타임에 보내 실제 알림 전달 경로로 만든다. update-task는 데스크톱을 띄운 채
    태스크 이름을 DB에서 바꾼다(태스크 이름을 바꾸는 모바일 API가 없다).

예:
  python3 qa/mobile/prepare_notification_desktop.py --run-dir qa-results/mobile/notifications-1
"""
from __future__ import annotations

import json
import sys
import threading
import time
import uuid
from datetime import datetime, timedelta, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from desktop_fixture import (  # noqa: E402
    IsolatedDesktop, MobileObserver, ProbeServer, base_argument_parser, create_repository, require_fresh_run_dir,
    run_fixture,
)

DESKTOP_HOOK_PORT = 39736
PROBE_PORT = 19737
LATEST_TASK_TITLE = "Maestro Open Task Latest"
LIVE_TASK_STATUS = "review"
WORKTREE_BRANCH = "qa/mobile-filter"
TARGET_TASK_ID = "00000000-0000-4000-8000-0000000000a1"
DESKTOP_STATE_TIMEOUT_S = 8
TARGET_PRESENCE_TIMEOUT_MS = 300
DESKTOP_PANEL_LABEL = "알림"
DESKTOP_MARK_ALL_LABEL = "모두 읽음 처리"
# (fixture 키, 제목, 연결 태스크 키, 이미 읽음)
SEEDED_NOTIFICATIONS = (
    ("mobileRead", "Maestro Mobile Read", None, False),
    ("desktopRead", "Maestro Desktop Read", None, False),
    ("openTask", "Maestro Open Latest Task", "target", False),
    ("alreadyRead", "Maestro Already Read", None, True),
)


def notification_body(title: str) -> str:
    return f"{title} fixture body"


def build_notification(title: str, task_id: str | None, is_read: bool, created_at: datetime) -> dict:
    """`AppNotification` 저장 모양(`app_settings.app_notifications`의 JSON 배열 원소)"""
    return {
        "id": str(uuid.uuid4()),
        "title": title,
        "body": notification_body(title),
        "taskId": task_id,
        "relativePath": f"/ko/task/{task_id}" if task_id else "/ko",
        "locale": "ko",
        "isRead": is_read,
        "createdAt": created_at.strftime("%Y-%m-%dT%H:%M:%S.000Z"),
        "dedupeKey": f"mobile-filter-notifications:{title}",
        "action": None,
    }


def register_projects(observer: MobileObserver, repositories_dir: Path) -> dict[str, dict]:
    """모바일 등록 경로로 두 저장소를 등록하고, 등록이 만든 프로젝트와 main 루트 태스크를 찾는다"""
    observer.request("POST", "/api/mobile/projects/scan", body={"rootPath": str(repositories_dir)})
    board = observer.board()
    tasks = [task for column in board["tasks"].values() for task in column]
    projects = {}
    for project in board["projects"]:
        root = next(task for task in tasks if task.get("projectId") == project["id"] and task.get("branchName") == "main")
        projects[Path(project["repoPath"]).name] = {"id": project["id"], "rootTaskId": root["id"]}
    return projects


def seed_while_stopped(desktop: IsolatedDesktop, alpha: dict, beta: dict, worktree_task_id: str) -> dict:
    """이름·브랜치 없는 태스크·알림 목록·설정은 데스크톱을 끈 사이에 넣어, 다시 켤 때 처음부터 그 상태로 읽게 한다"""
    now = datetime.now(timezone.utc)
    task_ids = {"target": TARGET_TASK_ID}
    notifications = {}
    stored = []
    for index, (key, title, task_key, is_read) in enumerate(SEEDED_NOTIFICATIONS):
        notification = build_notification(title, task_ids.get(task_key), is_read, now - timedelta(minutes=index + 1))
        notifications[key] = notification
        stored.append(notification)
    with desktop.database() as connection:
        connection.execute("UPDATE projects SET name = 'Maestro Alpha' WHERE id = ?", (alpha["id"],))
        connection.execute("UPDATE projects SET name = 'Maestro Beta' WHERE id = ?", (beta["id"],))
        for task_id, title in ((alpha["rootTaskId"], "Maestro Alpha Root"), (beta["rootTaskId"], "Maestro Beta Root"),
                               (worktree_task_id, "Maestro Alpha Worktree")):
            connection.execute("UPDATE kanban_tasks SET title = ? WHERE id = ?", (title, task_id))
        connection.execute("INSERT INTO kanban_tasks (id, title, project_id) VALUES (?, ?, ?)",
                           (TARGET_TASK_ID, "Maestro Open Task Original", beta["id"]))
    desktop.save_app_setting("app_notifications", json.dumps(stored, ensure_ascii=False))
    desktop.save_app_setting("background_sync_enabled", "false")
    # 필터·알림 검증에는 pane이 필요 없으므로 fixture가 tmux·herdr 세션을 만들지 않게 한다
    desktop.save_app_setting("default_session_type", "terminal")
    desktop.save_app_setting("notification_unread_only", "true")
    return notifications


class DesktopNotificationPanel:
    """데스크톱 알림 패널(안읽음만 보기). 알림 본문 글자가 패널에 있는지로 데스크톱이 그린 상태를 읽는다"""

    def __init__(self, desktop: IsolatedDesktop):
        self.control = desktop.control
        self.lock = threading.Lock()
        self.last_rendered_unread: frozenset[str] | None = None
        self.event_count = 0

    def is_shown(self, target: dict) -> bool:
        """`bounds`는 없는 대상을 15초 기다리므로, 짧게 끊는 `wait_for`로 지금 그려져 있는지만 본다"""
        response = self.control.send({"cmd": "wait_for", "target": target, "timeout_ms": TARGET_PRESENCE_TIMEOUT_MS},
                                     check=False)
        return bool(response.get("ok"))

    def is_open(self) -> bool:
        return self.is_shown({"role": "dialog", "name": DESKTOP_PANEL_LABEL})

    def open(self) -> None:
        if self.is_open():
            return
        self.control.send({"cmd": "click", "target": {"role": "button", "name": DESKTOP_PANEL_LABEL, "exact": True}})
        self.control.send({"cmd": "wait_for", "target": {"role": "dialog", "name": DESKTOP_PANEL_LABEL},
                           "timeout_ms": 5000})
        self.control.send({"cmd": "wait_idle"})

    def is_body_shown(self, body: str) -> bool:
        return self.is_shown({"text": body, "exact": True})

    def click_body(self, body: str) -> None:
        self.open()
        self.control.send({"cmd": "click", "target": {"text": body, "exact": True}})
        self.control.send({"cmd": "wait_idle"})

    def click_mark_all(self) -> None:
        self.open()
        self.control.send({"cmd": "click", "target": {"role": "button", "name": DESKTOP_MARK_ALL_LABEL, "exact": True}})
        self.control.send({"cmd": "wait_idle"})

    def observe(self, notifications: list[dict]) -> bool:
        """열린 패널이 저장된 안읽음 목록을 그대로 그렸는지. 새 상태를 처음 그려 보였을 때만 횟수를 올린다"""
        with self.lock:
            if not self.is_open():
                return False
            unread = frozenset(entry["body"] for entry in notifications if not entry["isRead"])
            is_matching = all(self.is_body_shown(entry["body"]) == (entry["body"] in unread) for entry in notifications)
            if is_matching and unread != self.last_rendered_unread:
                self.last_rendered_unread = unread
                self.event_count += 1
            return is_matching


class NotificationProbe:
    def __init__(self, desktop: IsolatedDesktop, observer: MobileObserver, panel: DesktopNotificationPanel,
                 fixture: dict):
        self.desktop = desktop
        self.observer = observer
        self.panel = panel
        self.fixture = fixture

    def routes(self) -> dict:
        actions = {
            "read-one": self.read_one, "read-all": self.read_all, "create": self.create_live_notification,
            "update-task": self.update_task, "create-task-notification": self.create_task_notification,
        }
        routes = {"/state": self.state}
        for name, action in actions.items():
            routes[f"/desktop/{name}"] = self._action_route(action)
        return routes

    def _action_route(self, action):
        def route(method: str, query: dict[str, str]) -> dict:
            if method != "POST":
                raise RuntimeError("desktop actions are POST only")
            action()
            return self.state("GET", query)
        return route

    def read_one(self) -> None:
        self.panel.click_body(self.fixture["notifications"]["desktopRead"]["body"])
        self.panel.open()

    def read_all(self) -> None:
        self.panel.click_mark_all()

    def emit_status_notification(self, task_id: str, title: str) -> dict:
        """보드 이벤트를 런타임에 보내 상태 변경 알림을 실제 전달 경로(설정 확인, 저장, 변경 신호)로 만든다"""
        self.desktop.control.send({"cmd": "board_event", "event": {
            "type": "task-status-changed", "projectName": "Maestro QA", "branchName": WORKTREE_BRANCH,
            "taskTitle": title, "description": None, "newStatus": LIVE_TASK_STATUS, "taskId": task_id,
        }})
        dedupe_key_suffix = f"{task_id}:{LIVE_TASK_STATUS}"
        deadline = time.monotonic() + DESKTOP_STATE_TIMEOUT_S
        while time.monotonic() < deadline:
            created = [entry for entry in self.observer.notifications()
                       if entry.get("taskId") == task_id and title in entry["body"]]
            if created:
                return created[0]
            time.sleep(0.2)
        raise RuntimeError(f"status notification was not stored: {dedupe_key_suffix}")

    def create_live_notification(self) -> None:
        created = self.emit_status_notification(self.fixture["tasks"]["worktreeId"], "Maestro Live Notice")
        self.fixture["liveNotificationTitle"] = created["title"]
        self.fixture["notifications"]["live"] = created

    def create_task_notification(self) -> None:
        # 모바일이 보드에서 태스크 상세를 열 때만 읽혀야 하므로, 알림함 흐름의 개수와 섞이지 않게 요청 시점에 만든다
        created = self.emit_status_notification(self.fixture["tasks"]["betaRootId"], "Maestro Task Detail Notice")
        self.fixture["notifications"]["taskDetail"] = created

    def update_task(self) -> None:
        with self.desktop.database() as connection:
            connection.execute("UPDATE kanban_tasks SET title = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                               (LATEST_TASK_TITLE, self.fixture["tasks"]["targetId"]))
        self.desktop.control.send({"cmd": "board_event", "event": {"type": "board-updated"}})

    def state(self, _method: str, query: dict[str, str]) -> dict:
        expected_unread = int(query["unread"]) if "unread" in query else None
        events_after = int(query["eventsAfter"]) if "eventsAfter" in query else None
        deadline = time.monotonic() + DESKTOP_STATE_TIMEOUT_S
        while True:
            notifications = self.observer.notifications()
            self.panel.observe(notifications)
            unread_count = sum(1 for entry in notifications if not entry["isRead"])
            is_unread_ok = expected_unread is None or unread_count == expected_unread
            is_event_ok = events_after is None or self.panel.event_count > events_after
            if (is_unread_ok and is_event_ok) or time.monotonic() >= deadline:
                break
            time.sleep(0.2)
        self.desktop.screenshot("desktop-latest")
        return {"notifications": notifications, "board": self.observer.board(), "fixture": self.fixture,
                "desktopEventCount": self.panel.event_count, "unreadCount": unread_count}


def prepare(desktop: IsolatedDesktop):
    run_dir = desktop.run_dir
    # 모바일 프로젝트 검색은 이름과 저장소 경로를 글자 순서(부분 수열)로 맞춘다. 긴 run 경로에는 "beta"의 글자가 차례로
    # 들어 있어 Alpha도 "Beta" 검색에 걸리므로, 저장소는 글자가 적은 격리 경로에 둔다
    repositories_dir = desktop.isolation_root / "repositories"
    for name in ("maestro-alpha", "maestro-beta"):
        create_repository(repositories_dir / name, f"# {name}\n", desktop.environment)
    desktop.start()
    # 등록이 만드는 루트 태스크가 tmux·herdr 세션을 띄우지 않게 등록 전에 기본 세션 종류를 정한다
    desktop.save_app_setting("default_session_type", "terminal")
    observer = MobileObserver(desktop, "Notification QA observer")
    projects = register_projects(observer, repositories_dir)
    alpha, beta = projects["maestro-alpha"], projects["maestro-beta"]
    worktree = observer.request("POST", "/api/mobile/tasks", body={
        "projectId": alpha["id"], "branchName": WORKTREE_BRANCH, "sessionType": "terminal"})["task"]
    if not worktree.get("worktreePath") or not Path(worktree["worktreePath"]).exists():
        raise RuntimeError(f"actual fixture worktree was not created: {worktree}")
    desktop.quit()
    notifications = seed_while_stopped(desktop, alpha, beta, worktree["id"])
    desktop.start()
    panel = DesktopNotificationPanel(desktop)
    panel.open()
    fixture = {
        "desktopApi": desktop.api_url,
        "probeUrl": f"http://127.0.0.1:{PROBE_PORT}",
        "projects": {"alpha": alpha, "beta": beta},
        "tasks": {"alphaRootId": alpha["rootTaskId"], "betaRootId": beta["rootTaskId"],
                  "worktreeId": worktree["id"], "targetId": TARGET_TASK_ID},
        "notifications": notifications,
        "latestTaskTitle": LATEST_TASK_TITLE,
        "liveNotificationTitle": "",
        "worktreePath": worktree["worktreePath"],
        "appDataDir": str(desktop.data_dir),
    }
    if not panel.observe(observer.notifications()):
        raise RuntimeError("desktop notification panel does not show the seeded unread notifications")
    desktop.screenshot("desktop-fixture")
    probe = ProbeServer(PROBE_PORT, NotificationProbe(desktop, observer, panel, fixture).routes(),
                        run_dir / "observations.jsonl")
    return fixture, probe.close


def main() -> None:
    arguments = base_argument_parser(__doc__).parse_args()
    run_dir = require_fresh_run_dir(arguments.run_dir)
    run_fixture(prepare, run_dir, Path(arguments.app), DESKTOP_HOOK_PORT, [PROBE_PORT])


if __name__ == "__main__":
    main()
