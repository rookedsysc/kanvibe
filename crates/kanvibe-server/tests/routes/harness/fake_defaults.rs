//! 가짜 서비스의 기본 응답. TS 테스트 파일들의 `createBridge()`·`fixture()` 기본값과 같은 값이다.

use serde_json::{Value, json};

pub const ACCOUNT_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

pub fn inbox_notification() -> Value {
    json!({
        "id": "notice-1", "title": "리뷰 요청", "body": "변경을 확인해 주세요", "taskId": "task-1",
        "isRead": false, "createdAt": "2026-11-01T00:00:00.000Z",
    })
}

pub fn default_response(name: &str, args: &Value) -> Value {
    if let Some(variant) = name.strip_prefix("feature.") {
        return feature_default(variant);
    }
    if let Some(route) = name.strip_prefix("task_cli.") {
        return task_cli_default(route);
    }
    if name.starts_with("pane_chat.") {
        return json!({ "success": true });
    }
    device_default(name, args)
        .or_else(|| board_default(name, args))
        .or_else(|| task_default(name))
        .or_else(|| ai_default(name, args))
        .unwrap_or(Value::Null)
}

fn device_default(name: &str, args: &Value) -> Option<Value> {
    Some(match name {
        "authorize_mobile_request" => match args.as_str() {
            Some("Bearer good") => json!("device-good"),
            Some("Bearer second") => json!("device-second"),
            _ => Value::Null,
        },
        "pair_mobile_device" => json!("paired-token"),
        "start_mobile_pairing" => json!({ "code": "654321", "expiresAt": 1_700_000_000_000_u64 }),
        "unpair_mobile_device_by_token"
        | "update_mobile_notification_subscription"
        | "delete_mobile_notification_subscription" => json!(true),
        "get_mobile_notifications" => json!([inbox_notification()]),
        "mark_mobile_notification_read" => {
            let mut notification = inbox_notification();
            notification["isRead"] = json!(true);
            notification
        }
        "get_mobile_notification_settings" => {
            json!({ "isEnabled": true, "enabledStatuses": ["review"] })
        }
        "update_mobile_notification_settings" => args.clone(),
        "read_hook_install_script" => json!("#!/bin/sh\n"),
        "update_hook_task_status" => json!({ "success": true }),
        _ => return None,
    })
}

fn board_default(name: &str, args: &Value) -> Option<Value> {
    Some(match name {
        "get_mobile_board" => json!({ "columns": [] }),
        "get_mobile_directories" => json!(["Documents", "Projects"]),
        "scan_mobile_projects" => json!({
            "registered": ["kanvibe"], "skipped": [], "errors": [], "worktreeTasks": [],
            "registeredWorktrees": [], "hooksSetup": [],
        }),
        "get_mobile_task" => {
            json!({ "id": "task-1", "title": "알림 태스크", "projectName": "kanvibe" })
        }
        "is_mobile_ai_session_provider" => json!(matches!(args.as_str(), Some("codex" | "hermes"))),
        "get_mobile_ai_sessions" => json!({
            "sessions": [{ "id": "session-1", "provider": "hermes" }], "sources": [], "nextCursor": null,
        }),
        "get_mobile_ai_session_detail" => json!({
            "sessionId": "session-1", "provider": "hermes",
            "messages": [{ "role": "assistant", "fullText": "기록 본문" }], "nextCursor": null,
        }),
        "get_sequenced_pane_chat_snapshot" => json!({
            "taskId": "task-1", "paneId": "%3", "canSend": true, "inputState": "ready", "targetToken": "opaque-token",
        }),
        "get_pane_chat_modes" => json!({ "%1": true }),
        "set_pane_chat_subscriptions" => behind_freshness(&args[1]),
        _ => return None,
    })
}

/// 구독 대상마다 `behind`로 답한다
fn behind_freshness(targets: &Value) -> Value {
    let panes: Vec<Value> = targets
        .as_array()
        .into_iter()
        .flatten()
        .map(|target| json!({ "taskId": target["taskId"], "paneId": target["paneId"], "freshness": "behind" }))
        .collect();
    Value::Array(panes)
}

fn task_default(name: &str) -> Option<Value> {
    Some(match name {
        "get_task_surfaces" => json!({ "panes": [] }),
        "create_mobile_terminal_tab" => json!({
            "surfaces": { "taskId": "task-1", "sessionType": "tmux", "canResize": true, "tabs": [] },
            "paneId": "%20",
        }),
        "execute_mobile_terminal_shortcut" => {
            json!({ "surfaces": { "tabs": [] }, "screen": "screen" })
        }
        "create_mobile_task" => json!({ "id": "task-1", "title": "feat/from-mobile" }),
        "update_mobile_task_status" => json!({ "id": "task-1", "status": "review" }),
        "delete_mobile_task" => json!(0),
        "remove_mobile_task_registration" | "is_mobile_task_session_alive" => json!(true),
        "close_mobile_pane" => json!({ "tabs": [] }),
        "get_mobile_pane_close_info" => json!({ "processNames": [] }),
        "get_mobile_project_branches" => json!(["main", "dev"]),
        "change_mobile_task_session_type" => json!({ "id": "task-1", "sessionType": "zellij" }),
        "render_mobile_diagram" => {
            json!({ "imageBase64": "jpeg", "width": 640, "height": 120, "mimeType": "image/jpeg" })
        }
        _ => return None,
    })
}

fn ai_default(name: &str, args: &Value) -> Option<Value> {
    Some(match name {
        "get_mobile_running_agent_panes" => json!([]),
        "get_mobile_live_ai_sessions" => json!({ "taskId": args, "sessions": [] }),
        "list_mobile_ai_accounts" => json!([{ "accountId": ACCOUNT_ID, "provider": "claude" }]),
        "add_mobile_ai_account" => json!({ "accountId": ACCOUNT_ID }),
        "remove_mobile_ai_account" => json!({ "outcome": "ok" }),
        "get_mobile_ai_usage_snapshot" => json!({ "accounts": [], "fetchedAt": "now" }),
        _ => return None,
    })
}

fn feature_default(variant: &str) -> Value {
    match variant {
        "SearchTasks" => json!({ "tasks": [{ "id": "task-1", "title": "searchable" }] }),
        "DoneTasks" => json!({ "tasks": [{ "id": "old-done" }], "doneTotal": 21 }),
        "MarkTaskNotificationsRead" => json!({ "updatedCount": 2 }),
        _ => json!({}),
    }
}

fn task_cli_default(route: &str) -> Value {
    match route {
        "Create" => json!({ "id": "created-task" }),
        "Delete" => json!({ "id": "deleted-task", "deleted": true }),
        "Register" => json!({ "id": "registered-task" }),
        "List" => json!([{ "id": "listed-task" }]),
        _ => json!({ "id": "prepared-task", "session": { "isPrepared": true } }),
    }
}
