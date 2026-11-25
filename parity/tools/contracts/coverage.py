"""inventory/01-backend.md §2의 경로 목록과 골든 케이스를 대조해 parity/contracts/coverage.md를 만든다."""

from __future__ import annotations

from pathlib import Path

# (inventory 절, route 키). route 키는 scenario.py 케이스의 "route" 값과 글자 그대로 같아야 한다
INVENTORY_ROUTES: list[tuple[str, str]] = [
    ("2.1 hook/CLI", "GET /api/hooks/health"),
    ("2.1 hook/CLI", "GET /api/install/hooks.sh"),
    ("2.1 hook/CLI", "POST /api/hooks/status"),
    ("2.1 hook/CLI", "POST /api/cli/tasks"),
    ("2.1 hook/CLI", "POST /api/cli/tasks/delete"),
    ("2.1 hook/CLI", "POST /api/cli/tasks/register"),
    ("2.1 hook/CLI", "POST /api/cli/tasks/list"),
    ("2.1 hook/CLI", "POST /api/cli/tasks/session"),
    ("2.2.1 mobile main", "POST /api/mobile/dev/pairing-code"),
    ("2.2.1 mobile main", "POST /api/mobile/pair"),
    ("2.2.1 mobile main", "DELETE /api/mobile/device"),
    ("2.2.1 mobile main", "GET /api/mobile/board"),
    ("2.2.1 mobile main", "GET /api/mobile/directories"),
    ("2.2.1 mobile main", "POST /api/mobile/projects/scan"),
    ("2.2.1 mobile main", "PUT /api/mobile/notifications"),
    ("2.2.1 mobile main", "DELETE /api/mobile/notifications"),
    ("2.2.1 mobile main", "GET /api/mobile/notifications"),
    ("2.2.1 mobile main", "GET /api/mobile/notification-settings"),
    ("2.2.1 mobile main", "PATCH /api/mobile/notification-settings"),
    ("2.2.1 mobile main", "POST /api/mobile/notifications/read-all"),
    ("2.2.1 mobile main", "PATCH /api/mobile/notifications/:id/read"),
    ("2.2.1 mobile main", "GET /api/mobile/tasks/:taskId/pane-chat"),
    ("2.2.1 mobile main", "GET /api/mobile/tasks/:taskId/pane-chat/suggestions"),
    ("2.2.1 mobile main", "POST /api/mobile/tasks/:taskId/pane-chat/prompt"),
    ("2.2.1 mobile main", "POST /api/mobile/tasks/:taskId/pane-chat/interrupt"),
    ("2.2.1 mobile main", "POST /api/mobile/tasks/:taskId/pane-chat/model-picker"),
    ("2.2.1 mobile main", "POST /api/mobile/tasks/:taskId/pane-chat/model-setting"),
    ("2.2.1 mobile main", "POST /api/mobile/tasks/:taskId/pane-chat/key"),
    ("2.2.1 mobile main", "POST /api/mobile/tasks/:taskId/pane-chat/attachments"),
    ("2.2.1 mobile main", "DELETE /api/mobile/tasks/:taskId/pane-chat/attachments"),
    ("2.2.1 mobile main", "GET /api/mobile/tasks/:taskId/ai-sessions/:provider/:sessionId/attachments/:attachmentId"),
    ("2.2.1 mobile main", "GET /api/mobile/tasks/:taskId/workspace-file"),
    ("2.2.1 mobile main", "GET /api/mobile/tasks/:taskId/ai-sessions"),
    ("2.2.1 mobile main", "GET /api/mobile/tasks/:taskId/ai-sessions/:provider/:sessionId"),
    ("2.2.1 mobile main", "GET /api/mobile/tasks/:taskId"),
    ("2.2.1 mobile main", "POST /api/mobile/tasks/:taskId/panes/:paneId/connection"),
    ("2.2.1 mobile main", "GET /api/mobile/tasks/:taskId/surfaces"),
    ("2.2.1 mobile main", "POST /api/mobile/tasks/:taskId/panes/:paneId/shortcuts"),
    ("2.2.1 mobile main", "POST /api/mobile/tasks/:taskId/tabs"),
    ("2.2.1 mobile main", "POST /api/mobile/tasks"),
    ("2.2.1 mobile main", "PATCH /api/mobile/tasks/:taskId/status"),
    ("2.2.1 mobile main", "PATCH /api/mobile/tasks/:taskId/session-type"),
    ("2.2.1 mobile main", "GET /api/mobile/tasks/:taskId/session-alive"),
    ("2.2.1 mobile main", "GET /api/mobile/projects/:projectId/branches"),
    ("2.2.2 mobile feature", "GET /api/mobile/tasks/search"),
    ("2.2.2 mobile feature", "GET /api/mobile/tasks/done"),
    ("2.2.2 mobile feature", "PATCH /api/mobile/tasks/:id/details"),
    ("2.2.2 mobile feature", "POST /api/mobile/tasks/:id/branch"),
    ("2.2.2 mobile feature", "POST /api/mobile/tasks/:id/pr/refresh"),
    ("2.2.2 mobile feature", "GET /api/mobile/hosts"),
    ("2.2.2 mobile feature", "PATCH /api/mobile/projects/:id/color"),
    ("2.2.2 mobile feature", "GET /api/mobile/projects/:id/tasks"),
    ("2.2.2 mobile feature", "GET /api/mobile/projects/:id/task"),
    ("2.2.2 mobile feature", "DELETE /api/mobile/projects/:id"),
    ("2.2.2 mobile feature", "GET /api/mobile/diff-stats"),
    ("2.2.2 mobile feature", "GET /api/mobile/tasks/:id/diff"),
    ("2.2.2 mobile feature", "GET /api/mobile/tasks/:id/files"),
    ("2.2.2 mobile feature", "PUT /api/mobile/tasks/:id/files"),
    ("2.2.2 mobile feature", "PATCH /api/mobile/tasks/:id/tabs/:tabId"),
    ("2.2.2 mobile feature", "POST /api/mobile/tasks/:id/tabs/:tabId/move"),
    ("2.2.2 mobile feature", "POST /api/mobile/tasks/:id/panes/:paneId/image"),
    ("2.2.2 mobile feature", "GET /api/mobile/pane-layouts"),
    ("2.2.2 mobile feature", "PUT /api/mobile/pane-layouts"),
    ("2.2.2 mobile feature", "DELETE /api/mobile/pane-layouts/:id"),
    ("2.2.2 mobile feature", "GET /api/mobile/settings"),
    ("2.2.2 mobile feature", "PATCH /api/mobile/settings"),
    ("2.2.2 mobile feature", "GET /api/mobile/sessions/change-targets"),
    ("2.2.2 mobile feature", "POST /api/mobile/sessions/change-all"),
    ("2.2.2 mobile feature", "POST /api/mobile/sync/run"),
    ("2.2.2 mobile feature", "GET /api/mobile/sync/status"),
    ("2.2.2 mobile feature", "POST /api/mobile/sync/start"),
    ("2.2.2 mobile feature", "GET /api/mobile/notifications/:id/review"),
    ("2.2.2 mobile feature", "POST /api/mobile/notifications/:id/review/delete"),
    ("2.2.2 mobile feature", "POST /api/mobile/tasks/:id/notifications/read"),
    ("2.2.3 diagram", "POST /api/mobile/diagrams/render"),
    ("2.2.4 AI", "GET /api/mobile/ai/running-panes"),
    ("2.2.4 AI", "GET /api/mobile/tasks/:id/live-ai-sessions"),
    ("2.2.4 AI", "GET /api/mobile/tasks/:id/agent-call-graph"),
    ("2.2.4 AI", "GET /api/mobile/ai/accounts"),
    ("2.2.4 AI", "POST /api/mobile/ai/accounts"),
    ("2.2.4 AI", "DELETE /api/mobile/ai/accounts/:accountId"),
    ("2.2.4 AI", "GET /api/mobile/ai/usage"),
    ("2.4 WebSocket", "WS /api/mobile/stream"),
    ("2.4 WebSocket", "WS /api/mobile/events"),
    ("2.4 WebSocket", "WS /api/mobile/ai/accounts/:accountId/login"),
]

# 성공 경로를 녹화하지 못한 경로와 그 이유. 오류·가드 경로는 녹화되어 있다
SUCCESS_PATH_GAPS = {
    "POST /api/mobile/dev/pairing-code": "Route is enabled only for source dev runs (KANVIBE_RENDERER_URL and not packaged). The recorder uses the built app, so the goldens record the disabled behavior: 401 without a token, 404 with one. Pairing codes come from the `issue_pairing_code` oracle instead.",
    "POST /api/mobile/tasks/:id/branch": "A success needs a task without a branch. No HTTP route creates one (mobile and CLI both require branchName); only desktop IPC can.",
    "POST /api/mobile/tasks/:id/pr/refresh": "Needs an authenticated `gh` and a GitHub remote. The isolated HOME has neither, so the golden records the 500 the TS app returns.",
    "POST /api/mobile/tasks/:id/panes/:paneId/image": "A success needs a task on an SSH host (the image is copied with scp). Recorded cases: validation 400, local-task 409, 3 MiB body limit.",
    "POST /api/mobile/tasks/:taskId/panes/:paneId/connection": "A direct-connection grant needs a remote Herdr task on an SSH host. Recorded cases: plaintext `connection:null` and TLS for a local task or an unknown task.",
    "GET /api/mobile/tasks/:taskId/pane-chat/suggestions": "Needs a live agent CLI in the pane to issue a targetToken. No agent CLI is installed in the contract environment.",
    "POST /api/mobile/tasks/:taskId/pane-chat/prompt": "Needs a live agent CLI and its targetToken (see suggestions).",
    "POST /api/mobile/tasks/:taskId/pane-chat/interrupt": "Needs a live agent CLI and its targetToken.",
    "POST /api/mobile/tasks/:taskId/pane-chat/model-picker": "Needs a live agent CLI and its targetToken.",
    "POST /api/mobile/tasks/:taskId/pane-chat/model-setting": "Needs a live agent CLI and its targetToken.",
    "POST /api/mobile/tasks/:taskId/pane-chat/key": "Needs a live agent CLI and its targetToken.",
    "POST /api/mobile/tasks/:taskId/pane-chat/attachments": "Needs a live agent CLI and its targetToken. The 14 MiB body limit is recorded with a 200 KiB body.",
    "DELETE /api/mobile/tasks/:taskId/pane-chat/attachments": "Needs a live agent CLI and a staged attachment.",
    "GET /api/mobile/tasks/:taskId/ai-sessions/:provider/:sessionId": "Needs Claude, Codex or Gemini session history files under the server's HOME. Recorded cases: 404 and validation 400.",
    "GET /api/mobile/tasks/:taskId/ai-sessions/:provider/:sessionId/attachments/:attachmentId": "Needs CLI session history with an attachment. Recorded cases: 404 and 400.",
    "GET /api/mobile/notifications/:id/review": "Needs a background-sync review notification, which is created only when GitHub reports merged PRs. Recorded cases: 409 (not a review notification) and 404.",
    "POST /api/mobile/notifications/:id/review/delete": "Needs a background-sync review notification (see above). Recorded cases: 400 and 409.",
}

NOT_RECORDABLE = [
    ("Loopback-only guard for hook paths (404 for a non-loopback peer)", "The server binds 127.0.0.1 unless LAN plaintext is allowed or was used. The recorder has no non-loopback peer that reaches the plaintext listener."),
    ("Per-SSH-host tunnel sockets and remote-host daemon `hooks-v1.sock` proxy (§2.1.3)", "Need an SSH host. Same 8 routes, plus the `sourceSshHosts` scope check, the 403/400/503/413/504 proxy errors and the `hook.request`/`hook.response` records."),
    ("Remote terminal agent `wss://<ssh-host>/stream/<grantId>` (§2.4.4)", "Runs on the SSH host and needs a remote Herdr grant."),
    ("WS 30 s ping/terminate, 1009 `authentication-pending` pre-auth overflow, 1001 `server-shutdown`", "Timing or shutdown behavior. Recording them would need a 30 s wait, a race against the auth recheck, or stopping the server mid-replay."),
    ("AI login `invalid-login-input` (1008) and `login-output-overflow` (1009)", "With no provider CLI, the login PTY exits right after `ready`, so a follow-up frame races the exit."),
    ("Push relay POST to `KANVIBE_MOBILE_PUSH_RELAY_URL`", "Outbound call to an external relay. The recorder sets no relay URL."),
    ("LAN plaintext bind decision and `lan_plaintext_last_request_at` bookkeeping", "Happens at startup, and only non-loopback traffic writes the setting."),
    ("`POST /api/mobile/tasks/:taskId/notifications/read` handled by the main router (`count`)", "Unreachable: the feature route shadows it, and the golden `mobile.notifications.task-read.ok` shows `updatedCount`."),
]


def _is_success(golden: dict) -> bool:
    expect = golden["expect"]
    if golden["transport"] == "ws":
        return expect.get("handshake", {}).get("status") == 101 and not any(
            frame.get("kind") == "close" and frame.get("code") not in (1000, 4001, 4002) for frame in expect.get("frames", []))
    status = expect.get("status")
    return isinstance(status, int) and 200 <= status < 300


def write_coverage(goldens: list[dict], output: Path) -> None:
    by_route: dict[str, list[dict]] = {}
    for golden in goldens:
        by_route.setdefault(golden["route"], []).append(golden)
    inventory_keys = {route for _, route in INVENTORY_ROUTES}
    covered = [route for _, route in INVENTORY_ROUTES if route in by_route]
    with_success = [route for route in covered if any(_is_success(golden) for golden in by_route[route])]
    lines = [
        "# Contract coverage",
        "",
        "Generated by `parity/tools/contracts/record.py` from `parity/contracts/cases/`. Do not edit by hand.",
        "Route list source: `.claude/plan/rust-gpui/2611/inventory/01-backend.md` §2 (8 hook/CLI routes, 74 reachable mobile HTTP routes, 3 desktop WebSockets).",
        "",
        f"- Routes in the inventory: {len(INVENTORY_ROUTES)}",
        f"- Routes with at least one golden case: {len(covered)}",
        f"- Routes whose success path is recorded: {len(with_success)}",
        f"- Golden cases: {len(goldens)}",
        "",
        "Every route has at least one error or guard case.",
        "\"Success\" means a 2xx response, or for WebSockets a 101 handshake with no error close.",
        "",
        "| § | Route | Success | Golden case IDs | Gap |",
        "|---|---|---|---|---|",
    ]
    for section, route in INVENTORY_ROUTES:
        cases = by_route.get(route, [])
        identifiers = ", ".join(f"`{golden['id']}`" for golden in cases) or "—"
        success = "yes" if route in with_success else ("no" if cases else "not recorded")
        gap = SUCCESS_PATH_GAPS.get(route, "") if route not in with_success else ""
        lines.append(f"| {section} | `{route}` | {success} | {identifiers} | {gap} |")
    extra_routes = sorted(route for route in by_route if route not in inventory_keys)
    lines += ["", "## Cross-cutting guards and listener behavior", "", "| Behavior | Golden case IDs |", "|---|---|"]
    for route in extra_routes:
        identifiers = ", ".join(f"`{golden['id']}`" for golden in by_route[route])
        lines.append(f"| {route} | {identifiers} |")
    lines += ["", "## Not recorded", "", "| Behavior | Reason |", "|---|---|"]
    for behavior, reason in NOT_RECORDABLE:
        lines.append(f"| {behavior} | {reason} |")
    output.write_text("\n".join(lines) + "\n")
