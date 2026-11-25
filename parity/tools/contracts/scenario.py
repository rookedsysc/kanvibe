"""녹화할 요청 시나리오. 순서가 곧 fixture 상태의 흐름이다(앞 케이스가 페어링·프로젝트·태스크를 만든다).

각 케이스는 기대값 없이 요청만 적는다. record.py가 TS 앱에 보내 본 실제 응답을 정규화해 `expect`로 채운다.
`volatile`은 녹화 때만 쓰는 경로별 변동값 지정이고, 결과는 골든의 `expect` 안에 매처로 남는다.
"""

from __future__ import annotations

JSON_HEADERS = {"Content-Type": "application/json"}
ZERO_TOKEN = "0" * 64
UNKNOWN_ID = "00000000-0000-4000-8000-000000000000"

# tmux는 새 창 이름을 처음 실행한 명령(tmux)에서 셸(bash)로 곧 바꾼다. 조회 시점에 따라 둘 중 하나가 보인다
TAB_NAME = "{{regex:tmux|bash}}"
SURFACE_VOLATILE = {
    "$.body.surfaces.tabs[*].name": TAB_NAME,
    "$.body.surfaces.tabs[*].panes[*].tabName": TAB_NAME,
}


def _auth(name: str | None) -> dict:
    return {"Authorization": f"Bearer {{{{var:{name}}}}}"} if name else {}


def http(case_id: str, route: str, method: str, path: str, *, group: str, title: str, listener: str = "plain",
         auth: str | None = None, body=None, body_text: str | None = None, body_fill: dict | None = None,
         headers: dict | None = None, capture: dict | None = None, volatile: dict | None = None,
         oracles: list | None = None, setup: list | None = None, delay_before: float | None = None) -> dict:
    request_headers = {**_auth(auth), **(headers or {})}
    request: dict = {"method": method, "path": path, "listener": listener, "headers": request_headers}
    if body is not None:
        request["headers"] = {**request_headers, **JSON_HEADERS}
        request["body"] = body
    if body_text is not None:
        request["headers"] = {**request_headers, **JSON_HEADERS}
        request["body_text"] = body_text
    if body_fill is not None:
        request["headers"] = {**request_headers, **JSON_HEADERS}
        request["body_fill"] = body_fill
    case = {"id": case_id, "group": group, "route": route, "title": title, "transport": "http", "request": request}
    for key, value in (("capture", capture), ("volatile", volatile), ("oracles", oracles), ("setup", setup), ("delay_before", delay_before)):
        if value:
            case[key] = value
    return case


def ws(case_id: str, route: str, path: str, *, group: str, title: str, auth: str | None = None, listener: str = "plain",
       steps: list | None = None, headers: dict | None = None, volatile: dict | None = None, capture: dict | None = None,
       setup: list | None = None, oracles: list | None = None) -> dict:
    case = {
        "id": case_id, "group": group, "route": route, "title": title, "transport": "ws",
        "request": {"path": path, "listener": listener, "headers": {**_auth(auth), **(headers or {})}},
        "steps": steps or [],
    }
    for key, value in (("capture", capture), ("volatile", volatile), ("oracles", oracles), ("setup", setup)):
        if value:
            case[key] = value
    return case


def oversize(prefix: str = '{"x":"', suffix: str = '"}', size: int = 64 * 1024 + 4096) -> dict:
    return {"bytes": size, "prefix": prefix, "suffix": suffix}


def command(request_id: str, name: str, **arguments) -> dict:
    return {"op": "send_text", "json": {"type": "command", "requestId": request_id, "command": name, **arguments}}


def reply(request_id: str, timeout: float = 20) -> dict:
    return {"op": "recv_until", "where": {"type": "command-result", "requestId": request_id}, "timeout": timeout}


TASK = "{{var:task_id}}"
PROJECT = "{{var:project_id}}"


def hook_cases() -> list[dict]:
    g = "hook"
    return [
        http("hook.health.ok", "GET /api/hooks/health", "GET", "/api/hooks/health", group=g, title="health probe on plaintext loopback"),
        http("hook.health.query-string-404", "GET /api/hooks/health", "GET", "/api/hooks/health?probe=1", group=g,
             title="hook router matches the exact request.url, so a query string falls through to 404"),
        http("hook.health.wrong-method-404", "GET /api/hooks/health", "POST", "/api/hooks/health", group=g, title="POST on the health path is not routed"),
        http("guard.origin.plain-403", "GUARD Origin", "GET", "/api/hooks/health", group="guard", headers={"Origin": "http://evil.example"},
             title="any request carrying an Origin header is rejected before routing"),
        http("guard.unknown-path.plain-404", "GUARD unknown path", "GET", "/api/unknown", group="guard", title="unknown non-mobile path on plaintext"),
        http("guard.tls.hook-path-404", "GUARD TLS hook path", "GET", "/api/hooks/health", group="guard", listener="tls",
             title="TLS listener carries only /api/mobile/*; hook paths answer 404"),
        {"id": "tls.certificate.identity", "group": "tls", "route": "TLS certificate", "transport": "tls-certificate",
         "title": "self-signed certificate on hook port + 10; fingerprint = sha256(DER) lowercase hex, pinned by the app",
         "capture": {"tls_fingerprint": "peer_certificate_sha256"}},
        http("install.hooks-sh.missing-token-404", "GET /api/install/hooks.sh", "GET", "/api/install/hooks.sh", group="install",
             title="install script without x-kanvibe-install-token"),
        http("install.hooks-sh.bad-token-404", "GET /api/install/hooks.sh", "GET", "/api/install/hooks.sh", group="install",
             headers={"x-kanvibe-install-token": "f" * 48}, title="install script with an unknown ticket"),
        http("install.hooks-sh.ok", "GET /api/install/hooks.sh", "GET", "/api/install/hooks.sh", group="install",
             headers={"x-kanvibe-install-token": "{{var:install_token}}"},
             oracles=[{"name": "issue_hook_install_ticket", "save": {"install_token": "token", "install_script_sha256": "scriptSha256"}}],
             setup=["oracle issue_hook_install_ticket with the fixed file list in README"],
             title="install script for a live ticket (text/plain; charset=utf-8)"),
    ]


def pairing_cases() -> list[dict]:
    g = "pairing"
    return [
        http("mobile.auth.missing-401", "GUARD mobile auth", "GET", "/api/mobile/board", group="guard", title="no Authorization header"),
        http("mobile.auth.unknown-token-401", "GUARD mobile auth", "GET", "/api/mobile/board", group="guard",
             headers={"Authorization": f"Bearer {ZERO_TOKEN}"}, title="well-formed but unknown device token"),
        http("mobile.auth.malformed-401", "GUARD mobile auth", "GET", "/api/mobile/board", group="guard",
             headers={"Authorization": "Token abc"}, title="non-Bearer scheme"),
        http("mobile.dev-pairing-code.disabled-unauth-401", "POST /api/mobile/dev/pairing-code", "POST", "/api/mobile/dev/pairing-code", group=g,
             title="dev pairing-code route is disabled outside source dev runs, so it falls through to device auth"),
        http("mobile.pair.bad-json-400", "POST /api/mobile/pair", "POST", "/api/mobile/pair", group=g, listener="tls", body_text="{",
             title="unparseable pairing body"),
        http("mobile.pair.wrong-code-401", "POST /api/mobile/pair", "POST", "/api/mobile/pair", group=g, listener="tls",
             body={"code": "000000", "deviceName": "Wrong"}, title="no live pairing code"),
        http("mobile.pair.ok-tls", "POST /api/mobile/pair", "POST", "/api/mobile/pair", group=g, listener="tls",
             body={"code": "{{var:pairing_code}}", "deviceName": "Contract Phone"},
             oracles=[{"name": "issue_pairing_code", "save": {"pairing_code": "code"}}],
             setup=["oracle issue_pairing_code (desktop settings screen equivalent)"],
             capture={"pair_token": "body.token"}, title="Flutter pairing over TLS: token returned, certificate fingerprint captured for pinning"),
        http("mobile.pair.code-consumed-401", "POST /api/mobile/pair", "POST", "/api/mobile/pair", group=g, listener="tls",
             body={"code": "{{var:pairing_code}}", "deviceName": "Again"}, title="a redeemed code cannot be reused"),
        http("mobile.pair.ok-plaintext-legacy", "POST /api/mobile/pair", "POST", "/api/mobile/pair", group=g,
             body={"code": "{{var:pairing_code_b}}", "deviceName": "Device B " + "x" * 80},
             oracles=[{"name": "issue_pairing_code", "save": {"pairing_code_b": "code"}}],
             capture={"device_b_token": "body.token"}, title="legacy plaintext pairing on the hook port (deviceName > 64 chars is truncated)"),
        http("mobile.pair.oversize-no-response", "POST /api/mobile/pair", "POST", "/api/mobile/pair", group=g, listener="tls",
             body_fill=oversize(), title="body over 64 KiB: server destroys the socket, no HTTP response"),
        http("guard.origin.tls-403", "GUARD Origin", "GET", "/api/mobile/board", group="guard", listener="tls", auth="pair_token",
             headers={"Origin": "https://evil.example"}, title="Origin guard also applies on TLS, before auth"),
        http("mobile.unknown-route-404", "GUARD mobile unknown path", "GET", "/api/mobile/does-not-exist", group="guard", auth="pair_token",
             title="authenticated but unknown mobile path"),
        http("mobile.dev-pairing-code.disabled-auth-404", "POST /api/mobile/dev/pairing-code", "POST", "/api/mobile/dev/pairing-code", group=g,
             auth="pair_token", title="disabled dev route with a valid token reaches the final 404"),
        http("mobile.tls.pinned-fingerprint", "GET /api/mobile/tasks/search", "GET", "/api/mobile/tasks/search", group=g, listener="tls",
             auth="pair_token", title="later TLS requests present the same certificate (pin stays valid)"),
    ]


def settings_cases() -> list[dict]:
    g = "settings"
    return [
        http("mobile.settings.get-defaults", "GET /api/mobile/settings", "GET", "/api/mobile/settings", group=g, auth="pair_token", title="fresh defaults"),
        http("mobile.settings.patch-session-type", "PATCH /api/mobile/settings", "PATCH", "/api/mobile/settings", group=g, auth="pair_token",
             body={"defaultSessionType": "tmux"}, title="switch default session type to tmux before any project is registered"),
        http("mobile.settings.patch-sort-and-sync", "PATCH /api/mobile/settings", "PATCH", "/api/mobile/settings", group=g, auth="pair_token",
             body={"sortPreference": {"keys": [{"field": "priority", "direction": "desc"}]}, "doneAlertDismissed": True,
                   "backgroundSync": {"isEnabled": False, "intervalMs": 120000}}, title="sort, done alert and background sync"),
        http("mobile.settings.patch-unknown-key-400", "PATCH /api/mobile/settings", "PATCH", "/api/mobile/settings", group=g, auth="pair_token",
             body={"theme": "dark"}, title="unknown key"),
        http("mobile.settings.patch-done-alert-false-400", "PATCH /api/mobile/settings", "PATCH", "/api/mobile/settings", group=g, auth="pair_token",
             body={"doneAlertDismissed": False}, title="done alert can only be dismissed"),
        http("mobile.settings.patch-interval-400", "PATCH /api/mobile/settings", "PATCH", "/api/mobile/settings", group=g, auth="pair_token",
             body={"backgroundSync": {"isEnabled": True, "intervalMs": 1000}}, title="interval below 1 minute"),
        http("mobile.settings.patch-bad-json-400", "PATCH /api/mobile/settings", "PATCH", "/api/mobile/settings", group=g, auth="pair_token",
             body_text="{", title="feature route body that is not JSON"),
        http("mobile.settings.patch-oversize-no-response", "PATCH /api/mobile/settings", "PATCH", "/api/mobile/settings", group=g, auth="pair_token",
             body_fill=oversize(), title="feature route body over 64 KiB"),
        http("mobile.notification-settings.get", "GET /api/mobile/notification-settings", "GET", "/api/mobile/notification-settings", group=g,
             auth="pair_token", title="notification settings defaults"),
        http("mobile.notification-settings.patch", "PATCH /api/mobile/notification-settings", "PATCH", "/api/mobile/notification-settings", group=g,
             auth="pair_token", body={"isEnabled": True, "enabledStatuses": ["progress", "pending", "review", "done"]}, title="enable done notifications"),
        http("mobile.notification-settings.patch-bad-json-400", "PATCH /api/mobile/notification-settings", "PATCH", "/api/mobile/notification-settings",
             group=g, auth="pair_token", body_text="{", title="unparseable body"),
        http("mobile.notification-settings.patch-invalid-400", "PATCH /api/mobile/notification-settings", "PATCH", "/api/mobile/notification-settings",
             group=g, auth="pair_token", body={"isEnabled": "yes", "enabledStatuses": ["nope"]}, title="invalid values"),
        http("mobile.notification-settings.wrong-method-404", "PATCH /api/mobile/notification-settings", "POST", "/api/mobile/notification-settings",
             group=g, auth="pair_token", body={}, title="unsupported method falls to 404"),
        http("mobile.hosts.get", "GET /api/mobile/hosts", "GET", "/api/mobile/hosts", group=g, auth="pair_token", title="no SSH hosts in the isolated HOME"),
        http("mobile.pane-layouts.get-empty", "GET /api/mobile/pane-layouts", "GET", "/api/mobile/pane-layouts", group=g, auth="pair_token", title="no layouts"),
        http("mobile.pane-layouts.put-global", "PUT /api/mobile/pane-layouts", "PUT", "/api/mobile/pane-layouts", group=g, auth="pair_token",
             body={"layoutType": "horizontal_2", "isGlobal": True, "panes": [{"position": 0, "command": ""}, {"position": 1, "command": "git status"}]},
             capture={"layout_id": "body.layout.id"}, title="save a global layout"),
        http("mobile.pane-layouts.put-wrong-pane-count-400", "PUT /api/mobile/pane-layouts", "PUT", "/api/mobile/pane-layouts", group=g, auth="pair_token",
             body={"layoutType": "quad", "isGlobal": True, "panes": [{"position": 0, "command": ""}]}, title="pane count mismatch"),
        http("mobile.pane-layouts.put-global-with-project-400", "PUT /api/mobile/pane-layouts", "PUT", "/api/mobile/pane-layouts", group=g,
             auth="pair_token", body={"layoutType": "single", "isGlobal": True, "projectId": "x", "panes": [{"position": 0, "command": ""}]},
             title="global layout cannot name a project"),
        http("mobile.pane-layouts.get-one", "GET /api/mobile/pane-layouts", "GET", "/api/mobile/pane-layouts", group=g, auth="pair_token", title="saved layout listed"),
        http("mobile.pane-layouts.delete-ok", "DELETE /api/mobile/pane-layouts/:id", "DELETE", "/api/mobile/pane-layouts/{{var:layout_id}}", group=g,
             auth="pair_token", title="delete layout"),
        http("mobile.pane-layouts.delete-unknown-404", "DELETE /api/mobile/pane-layouts/:id", "DELETE", f"/api/mobile/pane-layouts/{UNKNOWN_ID}", group=g,
             auth="pair_token", title="unknown layout"),
        http("mobile.sync.status", "GET /api/mobile/sync/status", "GET", "/api/mobile/sync/status", group=g, auth="pair_token", title="idle background sync"),
    ]


def project_cases() -> list[dict]:
    g = "projects"
    return [
        http("mobile.directories.ok", "GET /api/mobile/directories", "GET", "/api/mobile/directories?path={{var:demo_scan_root}}", group=g,
             auth="pair_token", title="list subdirectories of an absolute path"),
        http("mobile.directories.missing-dir", "GET /api/mobile/directories", "GET", "/api/mobile/directories?path={{var:fixture_root}}/missing", group=g,
             auth="pair_token", title="path that does not exist"),
        http("mobile.directories.unknown-ssh-host-400", "GET /api/mobile/directories", "GET",
             "/api/mobile/directories?path=/tmp&sshHost=nohost", group=g, auth="pair_token", title="unregistered sshHost"),
        http("mobile.projects.scan.missing-root-400", "POST /api/mobile/projects/scan", "POST", "/api/mobile/projects/scan", group=g, auth="pair_token",
             body={"rootPath": "  "}, title="blank rootPath"),
        http("mobile.projects.scan.bad-json-400", "POST /api/mobile/projects/scan", "POST", "/api/mobile/projects/scan", group=g, auth="pair_token",
             body_text="{", title="unparseable body"),
        http("mobile.projects.scan.ok-demo", "POST /api/mobile/projects/scan", "POST", "/api/mobile/projects/scan", group=g, auth="pair_token",
             body={"rootPath": "{{var:demo_scan_root}}"}, title="register the demo repository (also creates its main-branch task)"),
        http("mobile.projects.scan.ok-scratch", "POST /api/mobile/projects/scan", "POST", "/api/mobile/projects/scan", group=g, auth="pair_token",
             body={"rootPath": "{{var:scratch_scan_root}}"}, delay_before=1.2, title="register a second repository"),
        http("mobile.projects.scan.again-skipped", "POST /api/mobile/projects/scan", "POST", "/api/mobile/projects/scan", group=g, auth="pair_token",
             body={"rootPath": "{{var:demo_scan_root}}"}, title="rescan reports already-registered projects"),
        http("mobile.board.get", "GET /api/mobile/board", "GET", "/api/mobile/board", group=g, auth="pair_token",
             capture={"project_id": "body.board.projects[name=demo].id", "scratch_project_id": "body.board.projects[name=scratch].id",
                      "main_task_id": "body.board.tasks.todo[sessionName=demo-main].id"},
             title="board with two projects and their main-branch tasks"),
        http("mobile.board.get-deduplicated-tls", "GET /api/mobile/board", "GET", "/api/mobile/board?iconFormat=deduplicated", group=g, listener="tls",
             auth="pair_token", title="Flutter's board request: deduplicated icon format over TLS"),
        http("mobile.projects.branches.ok", "GET /api/mobile/projects/:projectId/branches", "GET", f"/api/mobile/projects/{PROJECT}/branches", group=g,
             auth="pair_token", title="local branches"),
        http("mobile.projects.branches.unknown", "GET /api/mobile/projects/:projectId/branches", "GET", f"/api/mobile/projects/{UNKNOWN_ID}/branches",
             group=g, auth="pair_token", title="unknown project"),
        http("mobile.projects.color.ok", "PATCH /api/mobile/projects/:id/color", "PATCH", f"/api/mobile/projects/{PROJECT}/color", group=g,
             auth="pair_token", body={"color": "#112233"}, title="set project color"),
        http("mobile.projects.color.bad-400", "PATCH /api/mobile/projects/:id/color", "PATCH", f"/api/mobile/projects/{PROJECT}/color", group=g,
             auth="pair_token", body={"color": "red"}, title="color must be #RRGGBB"),
        http("mobile.projects.color.unknown-404", "PATCH /api/mobile/projects/:id/color", "PATCH", f"/api/mobile/projects/{UNKNOWN_ID}/color", group=g,
             auth="pair_token", body={"color": "#112233"}, title="unknown project"),
        http("mobile.projects.tasks.ok", "GET /api/mobile/projects/:id/tasks", "GET", f"/api/mobile/projects/{PROJECT}/tasks", group=g,
             auth="pair_token", title="tasks of a project"),
        http("mobile.projects.tasks.unknown-404", "GET /api/mobile/projects/:id/tasks", "GET", f"/api/mobile/projects/{UNKNOWN_ID}/tasks", group=g,
             auth="pair_token", title="unknown project"),
        http("mobile.projects.task.ok", "GET /api/mobile/projects/:id/task", "GET", f"/api/mobile/projects/{PROJECT}/task?branch=main", group=g,
             auth="pair_token", title="task by branch"),
        http("mobile.projects.task.none", "GET /api/mobile/projects/:id/task", "GET", f"/api/mobile/projects/{PROJECT}/task?branch=nope", group=g,
             auth="pair_token", title="no task for that branch -> task:null"),
        http("mobile.projects.task.missing-branch-400", "GET /api/mobile/projects/:id/task", "GET", f"/api/mobile/projects/{PROJECT}/task", group=g,
             auth="pair_token", title="branch query missing"),
        http("mobile.projects.bad-id-encoding-400", "GET /api/mobile/projects/:id/tasks", "GET", "/api/mobile/projects/%E0%A4%A/tasks", group=g,
             auth="pair_token", title="undecodable path id on a feature route"),
    ]


def task_cases() -> list[dict]:
    g = "tasks"
    return [
        http("mobile.tasks.create.missing-fields-400", "POST /api/mobile/tasks", "POST", "/api/mobile/tasks", group=g, auth="pair_token",
             body={"projectId": PROJECT, "branchName": " "}, title="blank branch"),
        http("mobile.tasks.create.bad-json-400", "POST /api/mobile/tasks", "POST", "/api/mobile/tasks", group=g, auth="pair_token",
             body_text="{", title="unparseable body"),
        http("mobile.tasks.create.bad-ref-400", "POST /api/mobile/tasks", "POST", "/api/mobile/tasks", group=g, auth="pair_token",
             body={"projectId": PROJECT, "branchName": "bad..ref"}, title="unsafe git ref"),
        http("mobile.tasks.create.bad-session-type-400", "POST /api/mobile/tasks", "POST", "/api/mobile/tasks", group=g, auth="pair_token",
             body={"projectId": PROJECT, "branchName": "feat-x", "sessionType": "screen"}, title="unknown session type"),
        http("mobile.tasks.create.ok", "POST /api/mobile/tasks", "POST", "/api/mobile/tasks", group=g, auth="pair_token",
             body={"projectId": PROJECT, "branchName": "feat-a", "baseBranch": "main", "description": "  contract task  ", "priority": "medium",
                   "sessionType": "tmux"},
             capture={"task_id": "body.task.id", "task_worktree": "body.task.worktreePath"}, delay_before=1.2,
             title="create a tmux task (worktree + hooks + targets.json)"),
        http("mobile.tasks.get.ok", "GET /api/mobile/tasks/:taskId", "GET", f"/api/mobile/tasks/{TASK}", group=g, auth="pair_token",
             oracles=[{"name": "read_json_file", "args": {"path": "{{var:task_worktree}}/.kanvibe/targets.json"},
                       "save": {"task_token": "targets[0].token", "hook_client_id": "targets[0].clientId"}}],
             setup=["reads <worktree>/.kanvibe/targets.json written by the server to learn the HMAC task token"],
             title="task with projectName"),
        http("mobile.tasks.get.unknown-404", "GET /api/mobile/tasks/:taskId", "GET", f"/api/mobile/tasks/{UNKNOWN_ID}", group=g, auth="pair_token", title="unknown task"),
        http("mobile.tasks.search.ok", "GET /api/mobile/tasks/search", "GET", "/api/mobile/tasks/search", group=g, auth="pair_token",
             title="searchable tasks (feature route shadows tasks/:id)"),
        http("mobile.tasks.done.ok", "GET /api/mobile/tasks/done", "GET", "/api/mobile/tasks/done?offset=0&limit=5", group=g, auth="pair_token", title="done page"),
        http("mobile.tasks.done.bad-limit-400", "GET /api/mobile/tasks/done", "GET", "/api/mobile/tasks/done?limit=101", group=g, auth="pair_token", title="limit > 100"),
        http("mobile.tasks.details.ok", "PATCH /api/mobile/tasks/:id/details", "PATCH", f"/api/mobile/tasks/{TASK}/details", group=g, auth="pair_token",
             body={"description": "updated description", "priority": "high"}, delay_before=1.1, title="update description and priority"),
        http("mobile.tasks.details.bad-key-400", "PATCH /api/mobile/tasks/:id/details", "PATCH", f"/api/mobile/tasks/{TASK}/details", group=g,
             auth="pair_token", body={"title": "x"}, title="only description/priority allowed"),
        http("mobile.tasks.details.unknown-404", "PATCH /api/mobile/tasks/:id/details", "PATCH", f"/api/mobile/tasks/{UNKNOWN_ID}/details", group=g,
             auth="pair_token", body={"priority": None}, title="unknown task"),
        http("mobile.tasks.status.missing-400", "PATCH /api/mobile/tasks/:taskId/status", "PATCH", f"/api/mobile/tasks/{TASK}/status", group=g,
             auth="pair_token", body={}, title="status missing"),
        http("mobile.tasks.status.bad-enum-400", "PATCH /api/mobile/tasks/:taskId/status", "PATCH", f"/api/mobile/tasks/{TASK}/status", group=g,
             auth="pair_token", body={"status": "PROGRESS"}, title="mobile status is case-sensitive"),
        http("mobile.tasks.status.unknown-404", "PATCH /api/mobile/tasks/:taskId/status", "PATCH", f"/api/mobile/tasks/{UNKNOWN_ID}/status", group=g,
             auth="pair_token", body={"status": "review"}, title="unknown task"),
        http("mobile.tasks.status.bad-json-400", "PATCH /api/mobile/tasks/:taskId/status", "PATCH", f"/api/mobile/tasks/{TASK}/status", group=g,
             auth="pair_token", body_text="{", title="unparseable body"),
        http("mobile.tasks.status.ok", "PATCH /api/mobile/tasks/:taskId/status", "PATCH", f"/api/mobile/tasks/{TASK}/status", group=g,
             auth="pair_token", body={"status": "pending"}, delay_before=1.1, title="move task to pending (creates a notification)"),
        http("mobile.tasks.surfaces.ok", "GET /api/mobile/tasks/:taskId/surfaces", "GET", f"/api/mobile/tasks/{TASK}/surfaces", group=g,
             auth="pair_token", volatile=SURFACE_VOLATILE, title="tabs/panes of the tmux session (starts it when stopped)"),
        http("mobile.tasks.surfaces.minimal", "GET /api/mobile/tasks/:taskId/surfaces", "GET", f"/api/mobile/tasks/{TASK}/surfaces?metadata=minimal&tabId=@0",
             group=g, auth="pair_token", volatile=SURFACE_VOLATILE, title="minimal metadata for one tab"),
        http("mobile.tasks.surfaces.unknown-409", "GET /api/mobile/tasks/:taskId/surfaces", "GET", f"/api/mobile/tasks/{UNKNOWN_ID}/surfaces", group=g,
             auth="pair_token", title="unknown task"),
        http("mobile.tasks.session-alive.ok", "GET /api/mobile/tasks/:taskId/session-alive", "GET", f"/api/mobile/tasks/{TASK}/session-alive", group=g,
             auth="pair_token", title="session running after surfaces started it"),
        http("mobile.tasks.session-alive.unknown", "GET /api/mobile/tasks/:taskId/session-alive", "GET", f"/api/mobile/tasks/{UNKNOWN_ID}/session-alive",
             group=g, auth="pair_token", title="unknown task"),
        http("mobile.tasks.tabs.create", "POST /api/mobile/tasks/:taskId/tabs", "POST", f"/api/mobile/tasks/{TASK}/tabs", group=g, auth="pair_token",
             volatile=SURFACE_VOLATILE, capture={"second_pane_id": "body.paneId", "second_tab_id": "body.surfaces.tabs[1].id"},
             title="open a second terminal tab"),
        http("mobile.tasks.tabs.create-unknown-409", "POST /api/mobile/tasks/:taskId/tabs", "POST", f"/api/mobile/tasks/{UNKNOWN_ID}/tabs", group=g,
             auth="pair_token", title="unknown task"),
        http("mobile.tabs.rename.ok", "PATCH /api/mobile/tasks/:id/tabs/:tabId", "PATCH", f"/api/mobile/tasks/{TASK}/tabs/{{{{var:second_tab_id}}}}", group=g,
             auth="pair_token", body={"name": "logs"}, volatile={"$.body.tabs[0].name": TAB_NAME}, title="rename second tab"),
        http("mobile.tabs.rename.blank-400", "PATCH /api/mobile/tasks/:id/tabs/:tabId", "PATCH", f"/api/mobile/tasks/{TASK}/tabs/{{{{var:second_tab_id}}}}",
             group=g, auth="pair_token", body={"name": ""}, title="blank name"),
        http("mobile.tabs.rename.unknown-tab-404", "PATCH /api/mobile/tasks/:id/tabs/:tabId", "PATCH", f"/api/mobile/tasks/{TASK}/tabs/@99", group=g,
             auth="pair_token", body={"name": "x"}, title="unknown tab"),
        http("mobile.tabs.move.ok", "POST /api/mobile/tasks/:id/tabs/:tabId/move", "POST", f"/api/mobile/tasks/{TASK}/tabs/{{{{var:second_tab_id}}}}/move",
             group=g, auth="pair_token", body={"index": 0}, volatile={"$.body.tabs[*].name": "{{regex:tmux|bash|logs}}"}, title="move second tab first"),
        http("mobile.tabs.move.out-of-range-400", "POST /api/mobile/tasks/:id/tabs/:tabId/move", "POST",
             f"/api/mobile/tasks/{TASK}/tabs/{{{{var:second_tab_id}}}}/move", group=g, auth="pair_token", body={"index": 9}, title="index past the end"),
        http("mobile.tabs.move.bad-index-400", "POST /api/mobile/tasks/:id/tabs/:tabId/move", "POST",
             f"/api/mobile/tasks/{TASK}/tabs/{{{{var:second_tab_id}}}}/move", group=g, auth="pair_token", body={"index": -1}, title="negative index"),
        http("mobile.panes.shortcuts.ok", "POST /api/mobile/tasks/:taskId/panes/:paneId/shortcuts", "POST", f"/api/mobile/tasks/{TASK}/panes/%250/shortcuts",
             group=g, auth="pair_token", body={"action": "scroll-bottom"},
             volatile={"$.body.surfaces.tabs[*].name": "{{regex:tmux|bash|logs}}", "$.body.surfaces.tabs[*].panes[*].tabName": "{{regex:tmux|bash|logs}}",
                       "$.body.screen": "{{any:string}}"},
             title="tmux shortcut on pane %0"),
        http("mobile.panes.shortcuts.missing-action-400", "POST /api/mobile/tasks/:taskId/panes/:paneId/shortcuts", "POST",
             f"/api/mobile/tasks/{TASK}/panes/%250/shortcuts", group=g, auth="pair_token", body={}, title="action missing"),
        http("mobile.panes.shortcuts.unsupported-400", "POST /api/mobile/tasks/:taskId/panes/:paneId/shortcuts", "POST",
             f"/api/mobile/tasks/{TASK}/panes/%250/shortcuts", group=g, auth="pair_token", body={"action": "explode"}, title="unknown action"),
        http("mobile.panes.shortcuts.unknown-pane-409", "POST /api/mobile/tasks/:taskId/panes/:paneId/shortcuts", "POST",
             f"/api/mobile/tasks/{TASK}/panes/%2599/shortcuts", group=g, auth="pair_token", body={"action": "copy-mode"}, title="pane not in session"),
        http("mobile.panes.connection.plaintext-null", "POST /api/mobile/tasks/:taskId/panes/:paneId/connection", "POST",
             f"/api/mobile/tasks/{TASK}/panes/%250/connection", group=g, auth="pair_token",
             title="plaintext listener never grants a direct connection (Cache-Control: no-store)"),
        http("mobile.panes.connection.tls-local-task", "POST /api/mobile/tasks/:taskId/panes/:paneId/connection", "POST",
             f"/api/mobile/tasks/{TASK}/panes/%250/connection", group=g, listener="tls", auth="pair_token",
             title="TLS request for a local tmux task (no remote Herdr grant)"),
        http("mobile.panes.connection.tls-unknown-task", "POST /api/mobile/tasks/:taskId/panes/:paneId/connection", "POST",
             f"/api/mobile/tasks/{UNKNOWN_ID}/panes/%250/connection", group=g, listener="tls", auth="pair_token", title="TLS request for an unknown task"),
        http("mobile.panes.image.bad-mime-400", "POST /api/mobile/tasks/:id/panes/:paneId/image", "POST", f"/api/mobile/tasks/{TASK}/panes/%250/image",
             group=g, auth="pair_token", body={"mimeType": "image/gif", "base64": "R0lGOD=="}, title="unsupported mime type"),
        http("mobile.panes.image.local-task-409", "POST /api/mobile/tasks/:id/panes/:paneId/image", "POST", f"/api/mobile/tasks/{TASK}/panes/%250/image",
             group=g, auth="pair_token", body={"mimeType": "image/png", "base64": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=="},
             title="valid PNG but the task is not on an SSH host"),
        http("mobile.panes.image.oversize-no-response", "POST /api/mobile/tasks/:id/panes/:paneId/image", "POST", f"/api/mobile/tasks/{TASK}/panes/%250/image",
             group=g, auth="pair_token", body_fill=oversize('{"mimeType":"image/png","base64":"', '"}', 3 * 1024 * 1024 + 4096), title="body over 3 MiB"),
        http("mobile.panes.image.large-under-limit-400", "POST /api/mobile/tasks/:id/panes/:paneId/image", "POST", f"/api/mobile/tasks/{TASK}/panes/%250/image",
             group=g, auth="pair_token", body_fill=oversize('{"mimeType":"image/png","base64":"', '"}', 200 * 1024),
             title="200 KiB body is accepted by the 3 MiB route limit and rejected by validation instead"),
    ]


def workspace_cases() -> list[dict]:
    g = "workspace"
    return [
        http("mobile.workspace-file.ok", "GET /api/mobile/tasks/:taskId/workspace-file", "GET", f"/api/mobile/tasks/{TASK}/workspace-file?path=README.md",
             group=g, auth="pair_token", title="read a worktree file"),
        http("mobile.workspace-file.missing-path-400", "GET /api/mobile/tasks/:taskId/workspace-file", "GET", f"/api/mobile/tasks/{TASK}/workspace-file",
             group=g, auth="pair_token", title="path query missing"),
        http("mobile.workspace-file.outside-400", "GET /api/mobile/tasks/:taskId/workspace-file", "GET",
             f"/api/mobile/tasks/{TASK}/workspace-file?path=../../../../etc/hostname", group=g, auth="pair_token", title="path escapes the worktree"),
        http("mobile.workspace-file.not-found-404", "GET /api/mobile/tasks/:taskId/workspace-file", "GET",
             f"/api/mobile/tasks/{TASK}/workspace-file?path=missing.txt", group=g, auth="pair_token", title="missing file"),
        http("mobile.diff.empty", "GET /api/mobile/tasks/:id/diff", "GET", f"/api/mobile/tasks/{TASK}/diff", group=g, auth="pair_token", title="no changes yet"),
        http("mobile.diff.changed", "GET /api/mobile/tasks/:id/diff", "GET", f"/api/mobile/tasks/{TASK}/diff", group=g, auth="pair_token",
             oracles=[{"name": "write_file", "args": {"path": "{{var:task_worktree}}/notes/todo.txt", "content": "first line\nedited in worktree\n"}}],
             setup=["writes notes/todo.txt in the task worktree"], title="one modified file"),
        http("mobile.diff-stats.ok", "GET /api/mobile/diff-stats", "GET", "/api/mobile/diff-stats", group=g, auth="pair_token", title="per-task diff stats"),
        http("mobile.files.get.ok", "GET /api/mobile/tasks/:id/files", "GET", f"/api/mobile/tasks/{TASK}/files?path=notes/todo.txt", group=g,
             auth="pair_token", title="original and current content of a changed file"),
        http("mobile.files.get.not-changed-404", "GET /api/mobile/tasks/:id/files", "GET", f"/api/mobile/tasks/{TASK}/files?path=README.md", group=g,
             auth="pair_token", title="file not in the change list"),
        http("mobile.files.put.ok", "PUT /api/mobile/tasks/:id/files", "PUT", f"/api/mobile/tasks/{TASK}/files?path=notes/todo.txt", group=g,
             auth="pair_token", body={"content": "first line\nsaved from phone\n"}, title="save changed file"),
        http("mobile.files.put.nul-400", "PUT /api/mobile/tasks/:id/files", "PUT", f"/api/mobile/tasks/{TASK}/files?path=notes/todo.txt", group=g,
             auth="pair_token", body={"content": "bad\u0000content"}, title="NUL byte rejected"),
        http("mobile.files.put.large-under-limit", "PUT /api/mobile/tasks/:id/files", "PUT", f"/api/mobile/tasks/{TASK}/files?path=notes/todo.txt",
             group=g, auth="pair_token", body_fill=oversize('{"content":"', '"}', 200 * 1024),
             title="200 KiB body passes the 6 MiB route limit (default 64 KiB would reset)"),
        http("mobile.files.get.after-save", "GET /api/mobile/tasks/:id/files", "GET", f"/api/mobile/tasks/{TASK}/files?path=notes/todo.txt", group=g,
             auth="pair_token", volatile={"$.body.content": "{{regex:a{204786}}}"}, title="content after the large save (204786 x 'a')"),
    ]


def ai_cases() -> list[dict]:
    g = "ai"
    return [
        http("mobile.ai-sessions.list.ok", "GET /api/mobile/tasks/:taskId/ai-sessions", "GET", f"/api/mobile/tasks/{TASK}/ai-sessions", group=g,
             auth="pair_token", title="no AI CLI history in the isolated HOME"),
        http("mobile.ai-sessions.list.bad-limit-400", "GET /api/mobile/tasks/:taskId/ai-sessions", "GET", f"/api/mobile/tasks/{TASK}/ai-sessions?limit=0",
             group=g, auth="pair_token", title="limit out of range"),
        http("mobile.ai-sessions.list.source-ref-400", "GET /api/mobile/tasks/:taskId/ai-sessions", "GET",
             f"/api/mobile/tasks/{TASK}/ai-sessions?sourceRef=x", group=g, auth="pair_token", title="sourceRef is forbidden"),
        http("mobile.ai-sessions.list.unknown-task-404", "GET /api/mobile/tasks/:taskId/ai-sessions", "GET", f"/api/mobile/tasks/{UNKNOWN_ID}/ai-sessions",
             group=g, auth="pair_token", title="unknown task"),
        http("mobile.ai-sessions.detail.unknown-404", "GET /api/mobile/tasks/:taskId/ai-sessions/:provider/:sessionId", "GET",
             f"/api/mobile/tasks/{TASK}/ai-sessions/claude/abc?roles=user,assistant", group=g, auth="pair_token", title="unknown conversation"),
        http("mobile.ai-sessions.detail.bad-provider-400", "GET /api/mobile/tasks/:taskId/ai-sessions/:provider/:sessionId", "GET",
             f"/api/mobile/tasks/{TASK}/ai-sessions/bard/abc", group=g, auth="pair_token", title="unsupported provider"),
        http("mobile.ai-sessions.detail.bad-role-400", "GET /api/mobile/tasks/:taskId/ai-sessions/:provider/:sessionId", "GET",
             f"/api/mobile/tasks/{TASK}/ai-sessions/claude/abc?roles=user,robot", group=g, auth="pair_token", title="unknown role"),
        http("mobile.ai-sessions.attachment.bad-provider-400", "GET /api/mobile/tasks/:taskId/ai-sessions/:provider/:sessionId/attachments/:attachmentId",
             "GET", f"/api/mobile/tasks/{TASK}/ai-sessions/bard/abc/attachments/a1", group=g, auth="pair_token", title="unsupported provider"),
        http("mobile.ai-sessions.attachment.unknown-404", "GET /api/mobile/tasks/:taskId/ai-sessions/:provider/:sessionId/attachments/:attachmentId",
             "GET", f"/api/mobile/tasks/{TASK}/ai-sessions/claude/abc/attachments/a1", group=g, auth="pair_token", title="unknown attachment"),
        http("mobile.ai.running-panes.ok", "GET /api/mobile/ai/running-panes", "GET", "/api/mobile/ai/running-panes", group=g, auth="pair_token",
             title="no agent panes"),
        http("mobile.ai.live-sessions.ok", "GET /api/mobile/tasks/:id/live-ai-sessions", "GET", f"/api/mobile/tasks/{TASK}/live-ai-sessions", group=g,
             auth="pair_token", title="no live sessions"),
        http("mobile.ai.live-sessions.unknown-404", "GET /api/mobile/tasks/:id/live-ai-sessions", "GET", f"/api/mobile/tasks/{UNKNOWN_ID}/live-ai-sessions",
             group=g, auth="pair_token", title="unknown task"),
        http("mobile.ai.call-graph.missing-400", "GET /api/mobile/tasks/:id/agent-call-graph", "GET", f"/api/mobile/tasks/{TASK}/agent-call-graph",
             group=g, auth="pair_token", title="provider and sessionId required"),
        http("mobile.ai.call-graph.bad-provider-400", "GET /api/mobile/tasks/:id/agent-call-graph", "GET",
             f"/api/mobile/tasks/{TASK}/agent-call-graph?provider=bard&sessionId=s1", group=g, auth="pair_token", title="unsupported provider"),
        http("mobile.ai.call-graph.ok", "GET /api/mobile/tasks/:id/agent-call-graph", "GET",
             f"/api/mobile/tasks/{TASK}/agent-call-graph?provider=claude&sessionId=s1", group=g, auth="pair_token", title="graph for an unknown session"),
        http("mobile.ai.accounts.list-empty", "GET /api/mobile/ai/accounts", "GET", "/api/mobile/ai/accounts", group=g, auth="pair_token",
             title="no CLI accounts in the isolated HOME"),
        http("mobile.ai.accounts.add-ok", "POST /api/mobile/ai/accounts", "POST", "/api/mobile/ai/accounts", group=g, auth="pair_token",
             body={"provider": "claude", "accountName": "work"}, capture={"ai_account_id": "body.accountId"}, title="add a KanVibe-managed account (201)"),
        http("mobile.ai.accounts.add-duplicate-409", "POST /api/mobile/ai/accounts", "POST", "/api/mobile/ai/accounts", group=g, auth="pair_token",
             body={"provider": "claude", "accountName": "work"}, title="same name again"),
        http("mobile.ai.accounts.add-extra-key-400", "POST /api/mobile/ai/accounts", "POST", "/api/mobile/ai/accounts", group=g, auth="pair_token",
             body={"provider": "claude", "accountName": "x", "root": "/"}, title="unknown key"),
        http("mobile.ai.accounts.add-bad-json-400", "POST /api/mobile/ai/accounts", "POST", "/api/mobile/ai/accounts", group=g, auth="pair_token",
             body_text="{", title="SyntaxError -> 400"),
        http("mobile.ai.accounts.list-one", "GET /api/mobile/ai/accounts", "GET", "/api/mobile/ai/accounts", group=g, auth="pair_token", title="added account listed"),
        http("mobile.ai.usage.ok", "GET /api/mobile/ai/usage", "GET", "/api/mobile/ai/usage", group=g, auth="pair_token",
             title="usage snapshot without credentials"),
        http("mobile.ai.accounts.delete-bad-id-400", "DELETE /api/mobile/ai/accounts/:accountId", "DELETE", "/api/mobile/ai/accounts/not-hex", group=g,
             auth="pair_token", title="account id must be 64 hex"),
        http("mobile.ai.accounts.delete-unknown-404", "DELETE /api/mobile/ai/accounts/:accountId", "DELETE", f"/api/mobile/ai/accounts/{'a' * 64}", group=g,
             auth="pair_token", title="unknown account"),
        http("mobile.diagrams.render.ok", "POST /api/mobile/diagrams/render", "POST", "/api/mobile/diagrams/render", group="diagram", auth="pair_token",
             body={"definition": "graph TD; A-->B", "theme": "light", "width": 400},
             volatile={"$.body.imageBase64": "{{any:base64}}", "$.body.width": "{{any:int}}", "$.body.height": "{{any:int}}",
                       "$.body.naturalWidth": "{{any:int}}"},
             title="render mermaid to an image (bytes and size vary by renderer)"),
        http("mobile.diagrams.render.bad-theme-400", "POST /api/mobile/diagrams/render", "POST", "/api/mobile/diagrams/render", group="diagram",
             auth="pair_token", body={"definition": "graph TD; A-->B", "theme": "blue", "width": 400}, title="unknown theme"),
        http("mobile.diagrams.render.front-matter-400", "POST /api/mobile/diagrams/render", "POST", "/api/mobile/diagrams/render", group="diagram",
             auth="pair_token", body={"definition": "%%{init: {}}%%\ngraph TD; A-->B", "theme": "dark", "width": 400}, title="init directives are refused"),
        http("mobile.diagrams.render.bad-json-400", "POST /api/mobile/diagrams/render", "POST", "/api/mobile/diagrams/render", group="diagram",
             auth="pair_token", body_text="{", title="unparseable body"),
        http("mobile.diagrams.render.oversize-no-response", "POST /api/mobile/diagrams/render", "POST", "/api/mobile/diagrams/render", group="diagram",
             auth="pair_token", body_fill=oversize('{"definition":"', '","theme":"light","width":400}', 400 * 1024 + 4096), title="body over 400 KiB"),
    ]


def pane_chat_cases() -> list[dict]:
    g = "pane-chat"
    base = f"/api/mobile/tasks/{TASK}/pane-chat"
    bogus = {"targetToken": "bogus-token"}
    cases = [
        http("mobile.pane-chat.snapshot", "GET /api/mobile/tasks/:taskId/pane-chat", "GET", base + "?paneId=%250&wantsScreen=false", group=g,
             auth="pair_token", volatile={"$.body.snapshot.screen": "{{any:string}}"},
             title="snapshot for a pane without an agent (screen text depends on the shell prompt)"),
        http("mobile.pane-chat.snapshot-default", "GET /api/mobile/tasks/:taskId/pane-chat", "GET", base, group=g, auth="pair_token",
             title="snapshot without paneId"),
        http("mobile.pane-chat.agent", "GET /api/mobile/tasks/:taskId/pane-chat", "GET", base + "?paneId=%250&detail=agent", group=g, auth="pair_token",
             title="detail=agent returns only the provider"),
        http("mobile.pane-chat.suggestions.missing-token-400", "GET /api/mobile/tasks/:taskId/pane-chat/suggestions", "GET", base + "/suggestions",
             group=g, auth="pair_token", title="targetToken required"),
        http("mobile.pane-chat.suggestions.bogus-token", "GET /api/mobile/tasks/:taskId/pane-chat/suggestions", "GET",
             base + "/suggestions?targetToken=bogus-token&kind=file&query=RE&limit=5", group=g, auth="pair_token", title="stale target token"),
        http("mobile.pane-chat.prompt.missing-token-400", "POST /api/mobile/tasks/:taskId/pane-chat/prompt", "POST", base + "/prompt", group=g,
             auth="pair_token", body={"text": "hi"}, title="targetToken required"),
        http("mobile.pane-chat.prompt.bad-json-400", "POST /api/mobile/tasks/:taskId/pane-chat/prompt", "POST", base + "/prompt", group=g,
             auth="pair_token", body_text="{", title="unparseable body"),
        http("mobile.pane-chat.wrong-method-404", "GET /api/mobile/tasks/:taskId/pane-chat", "POST", base, group=g, auth="pair_token", body=bogus,
             title="POST without an action falls to 404"),
    ]
    for action, payload in (("prompt", {**bogus, "text": "hi", "attachmentIds": [], "requestId": "req-1"}), ("interrupt", bogus),
                            ("model-picker", bogus), ("model-setting", {**bogus, "change": {"kind": "model", "value": "x"}}),
                            ("key", {**bogus, "key": "enter"})):
        cases.append(http(f"mobile.pane-chat.{action}.bogus-token", f"POST /api/mobile/tasks/:taskId/pane-chat/{action}", "POST", f"{base}/{action}",
                          group=g, auth="pair_token", body=payload, title=f"{action} with a stale target token"))
    cases += [
        http("mobile.pane-chat.attachments.post-bogus-token", "POST /api/mobile/tasks/:taskId/pane-chat/attachments", "POST", base + "/attachments",
             group=g, auth="pair_token", body={**bogus, "attachment": {"name": "a.txt", "mimeType": "text/plain", "dataUrl": "data:text/plain;base64,aGk="}},
             title="stage attachment with a stale target token"),
        http("mobile.pane-chat.attachments.post-large-under-limit", "POST /api/mobile/tasks/:taskId/pane-chat/attachments", "POST", base + "/attachments",
             group=g, auth="pair_token",
             body_fill=oversize('{"targetToken":"bogus-token","attachment":{"name":"a.txt","mimeType":"text/plain","dataUrl":"data:text/plain;base64,', '"}}', 200 * 1024),
             title="200 KiB attachment body passes the 14 MiB route limit"),
        http("mobile.pane-chat.attachments.delete-bogus-token", "DELETE /api/mobile/tasks/:taskId/pane-chat/attachments", "DELETE", base + "/attachments",
             group=g, auth="pair_token", body={**bogus, "id": "att-1"}, title="remove attachment with a stale target token"),
        http("mobile.pane-chat.prompt.oversize-no-response", "POST /api/mobile/tasks/:taskId/pane-chat/prompt", "POST", base + "/prompt", group=g,
             auth="pair_token", body_fill=oversize('{"targetToken":"bogus-token","text":"', '"}'), title="non-attachment pane-chat body over 64 KiB"),
    ]
    return cases


def notification_cases() -> list[dict]:
    g = "notifications"
    return [
        http("mobile.notifications.list", "GET /api/mobile/notifications", "GET", "/api/mobile/notifications", group=g, auth="pair_token",
             capture={"notification_id": "body.notifications[0].id"}, title="notification from the pending status change"),
        http("mobile.notifications.read.ok", "PATCH /api/mobile/notifications/:id/read", "PATCH", "/api/mobile/notifications/{{var:notification_id}}/read",
             group=g, auth="pair_token", title="mark one read"),
        http("mobile.notifications.read.unknown-404", "PATCH /api/mobile/notifications/:id/read", "PATCH", f"/api/mobile/notifications/{UNKNOWN_ID}/read",
             group=g, auth="pair_token", title="unknown notification"),
        http("mobile.notifications.read-all", "POST /api/mobile/notifications/read-all", "POST", "/api/mobile/notifications/read-all", group=g,
             auth="pair_token", title="mark all read"),
        http("mobile.notifications.task-read.ok", "POST /api/mobile/tasks/:id/notifications/read", "POST", f"/api/mobile/tasks/{TASK}/notifications/read",
             group=g, auth="pair_token", title="feature route shadows the main handler: updatedCount, not count"),
        http("mobile.notifications.task-read.unknown-404", "POST /api/mobile/tasks/:id/notifications/read", "POST",
             f"/api/mobile/tasks/{UNKNOWN_ID}/notifications/read", group=g, auth="pair_token", title="unknown task"),
        http("mobile.notifications.review.not-review-409", "GET /api/mobile/notifications/:id/review", "GET",
             "/api/mobile/notifications/{{var:notification_id}}/review", group=g, auth="pair_token", title="not a background-sync review notification"),
        http("mobile.notifications.review.unknown-404", "GET /api/mobile/notifications/:id/review", "GET", f"/api/mobile/notifications/{UNKNOWN_ID}/review",
             group=g, auth="pair_token", title="unknown notification"),
        http("mobile.notifications.review-delete.bad-body-400", "POST /api/mobile/notifications/:id/review/delete", "POST",
             "/api/mobile/notifications/{{var:notification_id}}/review/delete", group=g, auth="pair_token", body={"taskIds": "x"}, title="taskIds must be an array"),
        http("mobile.notifications.review-delete.not-review-409", "POST /api/mobile/notifications/:id/review/delete", "POST",
             "/api/mobile/notifications/{{var:notification_id}}/review/delete", group=g, auth="pair_token", body={"taskIds": []},
             title="not a review notification"),
        http("mobile.notifications.subscribe.ok", "PUT /api/mobile/notifications", "PUT", "/api/mobile/notifications", group=g, auth="pair_token",
             body={"pushToken": "push-token-1", "platform": "android", "connectionId": "conn-1"}, title="register push subscription"),
        http("mobile.notifications.subscribe.bad-400", "PUT /api/mobile/notifications", "PUT", "/api/mobile/notifications", group=g, auth="pair_token",
             body={"pushToken": "has space", "platform": "android", "connectionId": "c"}, title="invalid push token"),
        http("mobile.notifications.subscribe.bad-json-400", "PUT /api/mobile/notifications", "PUT", "/api/mobile/notifications", group=g, auth="pair_token",
             body_text="{", title="unparseable body"),
        http("mobile.notifications.unsubscribe.ok", "DELETE /api/mobile/notifications", "DELETE", "/api/mobile/notifications", group=g, auth="pair_token",
             title="remove push subscription"),
    ]


def session_cases() -> list[dict]:
    g = "sessions"
    return [
        http("mobile.sessions.change-targets.ok", "GET /api/mobile/sessions/change-targets", "GET", "/api/mobile/sessions/change-targets?sessionType=tmux",
             group=g, auth="pair_token", title="tasks that would change"),
        http("mobile.sessions.change-targets.bad-400", "GET /api/mobile/sessions/change-targets", "GET", "/api/mobile/sessions/change-targets", group=g,
             auth="pair_token", title="sessionType required"),
        http("mobile.sessions.change-all.bad-400", "POST /api/mobile/sessions/change-all", "POST", "/api/mobile/sessions/change-all", group=g,
             auth="pair_token", body={"sessionType": "tmux"}, title="shouldCloseExistingSessions required"),
        http("mobile.sessions.change-all.ok", "POST /api/mobile/sessions/change-all", "POST", "/api/mobile/sessions/change-all", group=g,
             auth="pair_token", body={"sessionType": "tmux", "shouldCloseExistingSessions": False}, title="no-op change to the current type"),
        http("mobile.tasks.session-type.missing-400", "PATCH /api/mobile/tasks/:taskId/session-type", "PATCH", f"/api/mobile/tasks/{TASK}/session-type",
             group=g, auth="pair_token", body={}, title="sessionType required"),
        http("mobile.tasks.session-type.bad-400", "PATCH /api/mobile/tasks/:taskId/session-type", "PATCH", f"/api/mobile/tasks/{TASK}/session-type",
             group=g, auth="pair_token", body={"sessionType": "screen"}, title="unknown session type"),
        http("mobile.tasks.session-type.unknown-404", "PATCH /api/mobile/tasks/:taskId/session-type", "PATCH",
             f"/api/mobile/tasks/{UNKNOWN_ID}/session-type", group=g, auth="pair_token", body={"sessionType": "tmux"}, title="unknown task"),
        http("mobile.tasks.session-type.dependency-missing-409", "PATCH /api/mobile/tasks/:taskId/session-type", "PATCH",
             f"/api/mobile/tasks/{TASK}/session-type", group=g, auth="pair_token", body={"sessionType": "zellij"},
             title="zellij is not installed in the contract environment"),
        http("mobile.tasks.branch.already-branched-409", "POST /api/mobile/tasks/:id/branch", "POST", f"/api/mobile/tasks/{TASK}/branch", group=g,
             auth="pair_token", body={"projectId": PROJECT, "branchName": "feat-z", "baseBranch": "main", "sessionType": "tmux"},
             title="task already has a branch"),
        http("mobile.tasks.branch.bad-ref-400", "POST /api/mobile/tasks/:id/branch", "POST", f"/api/mobile/tasks/{TASK}/branch", group=g,
             auth="pair_token", body={"projectId": PROJECT, "branchName": "bad..ref", "baseBranch": "main", "sessionType": "tmux"}, title="unsafe ref"),
        http("mobile.tasks.pr-refresh.no-github", "POST /api/mobile/tasks/:id/pr/refresh", "POST", f"/api/mobile/tasks/{TASK}/pr/refresh", group=g,
             auth="pair_token", title="gh is unauthenticated in the isolated HOME"),
        http("mobile.tasks.pr-refresh.unknown-404", "POST /api/mobile/tasks/:id/pr/refresh", "POST", f"/api/mobile/tasks/{UNKNOWN_ID}/pr/refresh",
             group=g, auth="pair_token", title="unknown task"),
        http("mobile.sync.run", "POST /api/mobile/sync/run", "POST", "/api/mobile/sync/run", group=g, auth="pair_token",
             title="run background sync once (no GitHub remotes)"),
        http("mobile.sync.start", "POST /api/mobile/sync/start", "POST", "/api/mobile/sync/start", group=g, auth="pair_token",
             volatile={"$.body.status.phase": "{{any:string}}", "$.body.status.isRunning": "{{any:bool}}", "$.body.status.completedStages": "{{any:int}}"},
             title="start background sync (returns the status at start time)"),
    ]


def hook_task_cases() -> list[dict]:
    g = "hook-status"
    status_path = "/api/hooks/status"
    return [
        http("hook.status.ok", "POST /api/hooks/status", "POST", status_path, group=g, auth="task_token",
             headers={"x-kanvibe-client-id": "{{var:hook_client_id}}"}, body={"taskId": TASK, "status": "progress"}, delay_before=1.1,
             title="shell hook status update with the HMAC task token"),
        http("hook.status.case-insensitive", "POST /api/hooks/status", "POST", status_path, group=g, auth="task_token",
             body={"taskId": TASK, "status": "REVIEW", "agentSession": {"provider": "claude", "hookPid": 4242, "payloadBase64": "e30="}},
             title="status is lowercased; agentSession accepted"),
        http("hook.status.missing-fields-400", "POST /api/hooks/status", "POST", status_path, group=g, auth="task_token", body={"taskId": TASK},
             title="error bodies carry a numeric status field"),
        http("hook.status.bad-status-400", "POST /api/hooks/status", "POST", status_path, group=g, auth="task_token",
             body={"taskId": TASK, "status": "bogus"}, title="unknown status"),
        http("hook.status.bad-token-401", "POST /api/hooks/status", "POST", status_path, group=g, headers={"Authorization": f"Bearer {ZERO_TOKEN}"},
             body={"taskId": TASK, "status": "review"}, title="token does not match HMAC(secret, taskId)"),
        http("hook.status.missing-auth-401", "POST /api/hooks/status", "POST", status_path, group=g, body={"taskId": TASK, "status": "review"},
             title="no Authorization header"),
        http("hook.status.bad-json-500", "POST /api/hooks/status", "POST", status_path, group=g, auth="task_token", body_text="{",
             volatile={"$.body.error": "{{any:nonempty}}"}, title="bad JSON is a 500 whose error is the JS engine's parse message"),
        http("hook.status.oversize-no-response", "POST /api/hooks/status", "POST", status_path, group=g, auth="task_token", body_fill=oversize(),
             title="body over 64 KiB"),
        http("hook.status.query-string-404", "POST /api/hooks/status", "POST", status_path + "?x=1", group=g, auth="task_token",
             body={"taskId": TASK, "status": "review"}, title="exact url match"),
    ]


def cli_cases() -> list[dict]:
    g = "task-cli"
    return [
        http("cli.tasks.create.ok", "POST /api/cli/tasks", "POST", "/api/cli/tasks", group=g, auth="task_token",
             headers={"x-kanvibe-client-id": "{{var:hook_client_id}}"},
             body={"contextTaskId": TASK, "branchName": "cli-b", "baseBranch": "main", "title": "CLI B", "description": "from cli",
                   "priority": "low", "sessionType": "tmux"},
             capture={"cli_task_id": "body.data.id", "cli_task_worktree": "body.data.worktreePath"}, delay_before=1.2,
             title="task CLI creates a task with a prepared session (201)"),
        http("cli.tasks.create.duplicate-409", "POST /api/cli/tasks", "POST", "/api/cli/tasks", group=g, auth="task_token",
             body={"contextTaskId": TASK, "branchName": "cli-b", "baseBranch": "main"}, title="same branch again"),
        http("cli.tasks.create.missing-branch-400", "POST /api/cli/tasks", "POST", "/api/cli/tasks", group=g, auth="task_token",
             body={"contextTaskId": TASK, "baseBranch": "main"}, title="branchName required"),
        http("cli.tasks.create.missing-base-400", "POST /api/cli/tasks", "POST", "/api/cli/tasks", group=g, auth="task_token",
             body={"contextTaskId": TASK, "branchName": "cli-c"}, title="baseBranch required without fromTaskId"),
        http("cli.tasks.create.bad-token-401", "POST /api/cli/tasks", "POST", "/api/cli/tasks", group=g,
             headers={"Authorization": f"Bearer {ZERO_TOKEN}"}, body={"contextTaskId": TASK, "branchName": "cli-c", "baseBranch": "main"},
             title="wrong task token"),
        http("cli.tasks.create.bad-json-400", "POST /api/cli/tasks", "POST", "/api/cli/tasks", group=g, auth="task_token", body_text="{",
             title="JSON parse failure is 400 on CLI routes"),
        http("cli.tasks.create.non-object-400", "POST /api/cli/tasks", "POST", "/api/cli/tasks", group=g, auth="task_token", body_text="[1]",
             title="array body"),
        http("cli.tasks.create.oversize-no-response", "POST /api/cli/tasks", "POST", "/api/cli/tasks", group=g, auth="task_token", body_fill=oversize(),
             title="body over 64 KiB on a CLI route"),
        http("cli.tasks.list.ok", "POST /api/cli/tasks/list", "POST", "/api/cli/tasks/list", group=g, auth="task_token", body={"contextTaskId": TASK},
             title="tasks of the same project, createdAt ascending"),
        http("cli.tasks.list.missing-context-400", "POST /api/cli/tasks/list", "POST", "/api/cli/tasks/list", group=g, auth="task_token", body={},
             title="contextTaskId required"),
        http("cli.tasks.list.missing-auth-401", "POST /api/cli/tasks/list", "POST", "/api/cli/tasks/list", group=g, body={"contextTaskId": TASK},
             title="no Authorization header"),
        http("cli.tasks.list.query-string-404", "POST /api/cli/tasks/list", "POST", "/api/cli/tasks/list?x=1", group=g, auth="task_token",
             body={"contextTaskId": TASK}, title="exact url match"),
        http("cli.tasks.session.ok", "POST /api/cli/tasks/session", "POST", "/api/cli/tasks/session", group=g, auth="task_token",
             body={"contextTaskId": TASK}, title="prepare the context task's session"),
        http("cli.tasks.session.other-branch", "POST /api/cli/tasks/session", "POST", "/api/cli/tasks/session", group=g, auth="task_token",
             body={"contextTaskId": TASK, "branchName": "cli-b"}, title="prepare another task's session by branch"),
        http("cli.tasks.session.unknown-branch-404", "POST /api/cli/tasks/session", "POST", "/api/cli/tasks/session", group=g, auth="task_token",
             body={"contextTaskId": TASK, "branchName": "nope"}, title="unknown branch"),
        http("cli.tasks.register.unknown-worktree-404", "POST /api/cli/tasks/register", "POST", "/api/cli/tasks/register", group=g, auth="task_token",
             body={"contextTaskId": TASK, "branchName": "existing-branch"}, title="branch has no worktree yet"),
        http("cli.tasks.register.ok", "POST /api/cli/tasks/register", "POST", "/api/cli/tasks/register", group=g, auth="task_token",
             body={"contextTaskId": TASK, "branchName": "existing-branch"},
             oracles=[{"name": "git", "args": {"cwd": "{{var:demo_repo}}", "args": ["worktree", "add", "-q", "{{var:demo_repo}}__worktrees/existing-branch", "existing-branch"]}}],
             setup=["git worktree add <demo>__worktrees/existing-branch existing-branch (peer created the worktree)"],
             capture={"registered_task_id": "body.data.id"}, delay_before=1.2, title="register a worktree created outside KanVibe"),
        http("cli.tasks.register.missing-branch-400", "POST /api/cli/tasks/register", "POST", "/api/cli/tasks/register", group=g, auth="task_token",
             body={"contextTaskId": TASK}, title="branchName required"),
        http("cli.tasks.delete.not-confirmed-400", "POST /api/cli/tasks/delete", "POST", "/api/cli/tasks/delete", group=g, auth="task_token",
             body={"contextTaskId": TASK, "branchName": "cli-b"}, title="confirmed:true required"),
        http("cli.tasks.delete.default-branch-403", "POST /api/cli/tasks/delete", "POST", "/api/cli/tasks/delete", group=g, auth="task_token",
             body={"contextTaskId": TASK, "branchName": "main", "confirmed": True}, title="default-branch task cannot be deleted by CLI"),
        http("cli.tasks.delete.unknown-404", "POST /api/cli/tasks/delete", "POST", "/api/cli/tasks/delete", group=g, auth="task_token",
             body={"contextTaskId": TASK, "branchName": "nope", "confirmed": True}, title="unknown branch"),
        http("cli.tasks.delete.dirty-409", "POST /api/cli/tasks/delete", "POST", "/api/cli/tasks/delete", group=g, auth="task_token",
             body={"contextTaskId": TASK, "branchName": "existing-branch", "confirmed": True},
             oracles=[{"name": "write_file", "args": {"path": "{{var:demo_repo}}__worktrees/existing-branch/dirty.txt", "content": "uncommitted\n"}}],
             setup=["writes an untracked file into the existing-branch worktree"], title="dirty worktree refuses deletion without force"),
        http("cli.tasks.delete.ok", "POST /api/cli/tasks/delete", "POST", "/api/cli/tasks/delete", group=g, auth="task_token",
             body={"contextTaskId": TASK, "branchName": "cli-b", "confirmed": True, "propagateToPeers": False},
             oracles=[{"name": "read_json_file", "args": {"path": "{{var:cli_task_worktree}}/.kanvibe/targets.json"},
                       "save": {"cli_task_token": "targets[0].token"}}],
             title="delete the CLI-created task"),
        http("cli.tasks.delete.force-dirty", "POST /api/cli/tasks/delete", "POST", "/api/cli/tasks/delete", group=g, auth="task_token",
             body={"contextTaskId": TASK, "branchName": "existing-branch", "confirmed": True, "force": True}, title="force deletes a dirty worktree"),
        http("hook.status.deleted-task-404", "POST /api/hooks/status", "POST", "/api/hooks/status", group="hook-status", auth="cli_task_token",
             body={"taskId": "{{var:cli_task_id}}", "status": "review"}, title="valid HMAC token for a task that no longer exists"),
        http("cli.tasks.list.deleted-context-404", "POST /api/cli/tasks/list", "POST", "/api/cli/tasks/list", group=g, auth="cli_task_token",
             body={"contextTaskId": "{{var:cli_task_id}}"}, title="context task deleted"),
    ]


def websocket_cases() -> list[dict]:
    g = "ws"
    stream = f"/api/mobile/stream?taskId={TASK}&paneId=%250"
    return [
        ws("ws.upgrade.unknown-path", "WS upgrade guard", "/api/mobile/other-socket", group=g, auth="pair_token",
           title="upgrade on a non-WebSocket path: socket destroyed without a response"),
        ws("ws.events.no-auth-401", "WS /api/mobile/events", "/api/mobile/events", group=g, title="raw 'HTTP/1.1 401 Unauthorized' then close"),
        ws("ws.events.bad-token-401", "WS /api/mobile/events", "/api/mobile/events", group=g, headers={"Authorization": f"Bearer {ZERO_TOKEN}"},
           title="unknown device token"),
        ws("ws.events.origin-allowed", "WS /api/mobile/events", "/api/mobile/events", group=g, auth="pair_token",
           headers={"Origin": "http://evil.example"}, steps=[{"op": "recv"}, {"op": "close"}],
           title="no Origin check on upgrades: handshake succeeds with a valid token"),
        ws("ws.events.commands", "WS /api/mobile/events", "/api/mobile/events", group=g, auth="pair_token",
           steps=[
               {"op": "recv"},
               command("r1", "get-pane-chat-modes", taskId=TASK), reply("r1"),
               command("r2", "start-session", taskId=TASK), reply("r2"),
               command("r3", "launch-rockets"), reply("r3"),
               {"op": "send_text", "json": {"type": "command", "requestId": "r4", "command": 7}}, reply("r4"),
               command("r5", "remove-task-registration", taskId=UNKNOWN_ID), reply("r5"),
               command("r6", "get-tab-close-info", taskId=TASK, paneId="%0"), reply("r6"),
               command("r7", "set-pane-chat-mode", taskId=TASK, paneId="%0"), reply("r7"),
               command("r8", "delete-task", taskId=UNKNOWN_ID), reply("r8"),
               command("r9", "start-session", taskId=UNKNOWN_ID), reply("r9"),
               {"op": "send_text", "text": "not json"},
               {"op": "send_binary", "json": {"type": "command", "requestId": "rb", "command": "get-pane-chat-modes", "taskId": TASK}},
               {"op": "send_text", "json": {"type": "command", "requestId": "big", "command": "get-pane-chat-modes", "taskId": "x" * 1100}},
               command("r10", "set-pane-chat-subscription", panes=[{"taskId": TASK, "paneId": "%0", "detail": "chat", "wantsScreen": False}]), reply("r10"),
               command("r11", "set-pane-chat-subscription", panes=[]), reply("r11"),
               {"op": "close"},
           ],
           volatile={"$.frames[2].json.surfaces.tabs[*].name": "{{regex:tmux|bash|logs}}",
                     "$.frames[2].json.surfaces.tabs[*].panes[*].tabName": "{{regex:tmux|bash|logs}}",
                     "$.frames[6].json.processNames": "{{any:array}}"},
           title="connected frame + command round trips; malformed, binary and >1 KiB frames get no reply"),
        ws("ws.events.task-status-broadcast", "WS /api/mobile/events", "/api/mobile/events", group=g, auth="pair_token",
           steps=[
               {"op": "recv"},
               {"op": "http", "request": {"method": "POST", "path": "/api/hooks/status", "listener": "plain",
                                          "headers": {"Authorization": "Bearer {{var:task_token}}", **JSON_HEADERS},
                                          "body": {"taskId": TASK, "status": "pending"}}},
               {"op": "recv_until", "where": {"type": "task-status-changed"}, "timeout": 10},
               {"op": "close"},
           ],
           title="hook status change is pushed to event sockets"),
        ws("ws.events.tls", "WS /api/mobile/events", "/api/mobile/events", group=g, listener="tls", auth="pair_token",
           steps=[{"op": "recv"}, command("t1", "get-pane-chat-modes", taskId=TASK), reply("t1"), {"op": "close"}],
           title="events over WSS with the pinned certificate"),
        ws("ws.stream.missing-params-400", "WS /api/mobile/stream", "/api/mobile/stream?taskId=" + TASK, group=g, auth="pair_token",
           title="paneId missing: raw 400"),
        ws("ws.stream.no-auth-401", "WS /api/mobile/stream", stream, group=g, title="no token"),
        ws("ws.stream.unknown-task", "WS /api/mobile/stream", f"/api/mobile/stream?taskId={UNKNOWN_ID}&paneId=%250", group=g, auth="pair_token",
           steps=[{"op": "expect_close", "timeout": 10}], title="unknown task closes with 4000"),
        ws("ws.stream.mirror", "WS /api/mobile/stream", stream, group=g, auth="pair_token",
           steps=[
               {"op": "send_binary", "json": {"type": "resize", "cols": 80, "rows": 24}},
               {"op": "recv", "timeout": 10},
               # TS 앱은 resize 직후(약 100 ms) 미러를 다시 잡는 동안 들어온 출력을 흘릴 수 있어, 화면이 가라앉은 뒤에 입력한다
               {"op": "sleep", "seconds": 2},
               {"op": "send_text", "text": "echo kv-contract-$((6*7))\r"},
               {"op": "recv_until", "contains": "kv-contract-42", "timeout": 10},
               {"op": "send_binary", "json": {"type": "refresh-history"}},
               {"op": "send_binary", "json": {"type": "resize", "cols": 0, "rows": 24}},
               {"op": "send_text", "text": "echo kv-alive-$((7*6))\r"},
               {"op": "recv_until", "contains": "kv-alive-42", "timeout": 10},
               {"op": "close"},
           ],
           volatile={"$.frames[0].text": "{{any:string}}", "$.frames[1].text": "{{contains:kv-contract-42}}",
                     "$.frames[2].text": "{{contains:kv-alive-42}}"},
           title="resize claims the viewport and returns a snapshot; keystrokes echo; invalid resize ignored"),
        ws("ws.stream.tls", "WS /api/mobile/stream", stream, group=g, listener="tls", auth="pair_token",
           steps=[{"op": "send_binary", "json": {"type": "resize", "cols": 100, "rows": 30}}, {"op": "recv", "timeout": 10}, {"op": "close"}],
           volatile={"$.frames[0].text": "{{any:string}}"}, title="stream over WSS"),
        ws("ws.ai-login.bad-account-1008", "WS /api/mobile/ai/accounts/:accountId/login", "/api/mobile/ai/accounts/not-hex/login", group=g,
           auth="pair_token", steps=[{"op": "recv"}, {"op": "expect_close"}], title="account id must be 64 hex"),
        ws("ws.ai-login.bad-size-1008", "WS /api/mobile/ai/accounts/:accountId/login", "/api/mobile/ai/accounts/{{var:ai_account_id}}/login?cols=1&rows=24",
           group=g, auth="pair_token", steps=[{"op": "recv"}, {"op": "expect_close"}], title="cols below 2"),
        ws("ws.ai-login.unknown-account", "WS /api/mobile/ai/accounts/:accountId/login", f"/api/mobile/ai/accounts/{'a' * 64}/login", group=g,
           auth="pair_token", steps=[{"op": "recv"}, {"op": "expect_close"}], title="unknown account"),
        ws("ws.ai-login.no-auth-401", "WS /api/mobile/ai/accounts/:accountId/login", "/api/mobile/ai/accounts/{{var:ai_account_id}}/login", group=g,
           title="no token"),
        ws("ws.ai-login.cli-missing", "WS /api/mobile/ai/accounts/:accountId/login", "/api/mobile/ai/accounts/{{var:ai_account_id}}/login?cols=80&rows=24",
           group=g, auth="pair_token",
           steps=[{"op": "recv", "timeout": 10}, {"op": "recv", "timeout": 10}, {"op": "recv", "timeout": 10}, {"op": "expect_close", "timeout": 10}],
           volatile={"$.frames[1].json.data": "{{any:string}}"},
           title="login PTY: ready, output, exit (provider CLI is absent in the contract environment), close 1000"),
    ]


def teardown_cases() -> list[dict]:
    return [
        ws("ws.events.unpair-closes-4001", "DELETE /api/mobile/device", "/api/mobile/events", group="pairing", auth="device_b_token",
           steps=[
               {"op": "recv"},
               {"op": "http", "request": {"method": "DELETE", "path": "/api/mobile/device", "listener": "plain",
                                          "headers": {"Authorization": "Bearer {{var:device_b_token}}"}}},
               {"op": "expect_close", "timeout": 10},
           ],
           title="device B unpairs itself: 200, then its open event socket closes 4001 device-unpaired"),
        http("mobile.device.delete-again-401", "DELETE /api/mobile/device", "DELETE", "/api/mobile/device", group="pairing", auth="device_b_token",
             title="token already removed"),
        ws("ws.stream.session-changed-4002", "WS /api/mobile/stream", f"/api/mobile/stream?taskId={TASK}&paneId=%250", group="ws", auth="pair_token",
           steps=[
               {"op": "sleep", "seconds": 1},
               {"op": "http", "request": {"method": "PATCH", "path": f"/api/mobile/tasks/{TASK}/session-type", "listener": "plain",
                                          "headers": {"Authorization": "Bearer {{var:pair_token}}", **JSON_HEADERS},
                                          "body": {"sessionType": "terminal", "shouldCloseExistingSession": True}}},
               {"op": "expect_close", "timeout": 15},
           ],
           title="changing the task's session type closes the mirror with 4002 session-changed"),
        http("mobile.tasks.create.second", "POST /api/mobile/tasks", "POST", "/api/mobile/tasks", group="tasks", auth="pair_token",
             body={"projectId": PROJECT, "branchName": "feat-del", "baseBranch": "main", "sessionType": "tmux"},
             capture={"delete_task_id": "body.task.id"}, delay_before=1.2, title="task to delete over the events socket"),
        ws("ws.events.delete-task", "WS /api/mobile/events", "/api/mobile/events", group="ws", auth="pair_token",
           steps=[{"op": "recv"}, command("d1", "delete-task", taskId="{{var:delete_task_id}}", isForced=True), reply("d1", 60), {"op": "close"}],
           title="delete-task command"),
        http("mobile.projects.delete.ok", "DELETE /api/mobile/projects/:id", "DELETE", "/api/mobile/projects/{{var:scratch_project_id}}", group="projects",
             auth="pair_token", title="unregister the scratch project"),
        http("mobile.projects.delete.unknown-404", "DELETE /api/mobile/projects/:id", "DELETE", f"/api/mobile/projects/{UNKNOWN_ID}", group="projects",
             auth="pair_token", title="unknown project"),
        http("mobile.ai.accounts.delete-ok", "DELETE /api/mobile/ai/accounts/:accountId", "DELETE", "/api/mobile/ai/accounts/{{var:ai_account_id}}",
             group="ai", auth="pair_token", title="remove the KanVibe-managed account"),
        http("mobile.board.final", "GET /api/mobile/board", "GET", "/api/mobile/board", group="projects", auth="pair_token",
             title="board after all mutations"),
        http("mobile.device.delete-ok", "DELETE /api/mobile/device", "DELETE", "/api/mobile/device", group="pairing", auth="pair_token",
             title="primary device unpairs itself"),
        http("mobile.device.after-unpair-401", "GET /api/mobile/board", "GET", "/api/mobile/board", group="guard", auth="pair_token",
             title="removed token is rejected"),
    ]


def build_cases() -> list[dict]:
    sections = [hook_cases(), pairing_cases(), settings_cases(), project_cases(), task_cases(), workspace_cases(), ai_cases(),
                pane_chat_cases(), session_cases(), hook_task_cases(), notification_cases(), cli_cases(), websocket_cases(), teardown_cases()]
    cases = [case for section in sections for case in section]
    identifiers = [case["id"] for case in cases]
    duplicates = {identifier for identifier in identifiers if identifiers.count(identifier) > 1}
    if duplicates:
        raise ValueError(f"duplicate case ids: {sorted(duplicates)}")
    return cases
