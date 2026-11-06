use serde_json::{json, Value};
use tauri::{AppHandle, Listener, Manager};
fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
fn list<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v[k].as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn keys(v: &Value, field: &str, parts: &[&str]) -> String {
    let mut entries: Vec<_> = list(v, field)
        .iter()
        .map(|v| {
            parts
                .iter()
                .map(|key| text(v, key))
                .collect::<Vec<_>>()
                .join(":")
        })
        .collect();
    entries.sort();
    entries.join("|")
}
pub fn initialize(app: &AppHandle) {
    let handle = app.clone();
    app.listen("kanvibe:board-event", move |event| {
        let Ok(payload) = serde_json::from_str::<Value>(event.payload()) else {
            return;
        };
        let app = handle.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let result = (|| -> Result<(), String> {
                let settings =
                    crate::core::dispatch(&app, "appSettings", "getNotificationSettings", vec![])?;
                if settings["isEnabled"] != true {
                    return Ok(());
                }
                let requested = if payload["type"] == "task-status-changed" {
                    &payload["newStatus"]
                } else {
                    &payload["requestedStatus"]
                };
                if payload["type"] != "background-sync-review-needed"
                    && !list(&settings, "enabledStatuses").contains(requested)
                {
                    return Ok(());
                }
                let locale = app
                    .get_webview_window("main")
                    .and_then(|w| w.url().ok())
                    .and_then(|u| u.fragment().map(String::from))
                    .and_then(|r| r.split('/').find(|s| !s.is_empty()).map(String::from))
                    .unwrap_or("ko".into());
                if let Some(notification) = prepare(&payload, &locale) {
                    crate::runtime::notification(&app, "showNotification", vec![notification])?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                eprintln!("[kanvibe] desktop board notification: {error}");
            }
        });
    });
}
fn locale(value: &str) -> &str {
    if value.starts_with("en") {
        "en"
    } else if value.starts_with("zh") {
        "zh"
    } else {
        "ko"
    }
}
pub fn prepare(payload: &Value, requested_locale: &str) -> Option<Value> {
    let lang = locale(requested_locale);
    let task = text(payload, "taskId");
    match text(payload, "type") {
        "task-status-changed" => {
            let title = format!(
                "{} — {}",
                text(payload, "projectName"),
                text(payload, "branchName")
            );
            let status = text(payload, "newStatus");
            let task_title = text(payload, "taskTitle");
            let mut body = match lang {
                "en" => format!("{task_title}: changed to {status}"),
                "zh" => format!("{task_title}: 已变更为{status}"),
                _ => format!("{task_title}: {status}로 변경"),
            };
            let description = text(payload, "description");
            if !description.is_empty() {
                body.push('\n');
                body.push_str(description);
            }
            Some(
                json!({"title":title,"body":body,"taskId":task,"locale":requested_locale,"relativePath":format!("/{requested_locale}/task/{task}"),"dedupeKey":format!("task-status:{task}:{status}")}),
            )
        }
        "hook-status-target-missing" => {
            let status = text(payload, "requestedStatus");
            let body = match lang {
                "en" => {
                    format!("Failed to change status to {status}.\nNo matching task was found.")
                }
                "zh" => format!("未能变更为 {status} 状态。\n未找到匹配的任务。"),
                _ => {
                    format!("{status} 상태로 변경하지 못했습니다.\n연결된 작업을 찾지 못했습니다.")
                }
            };
            Some(
                json!({"title":format!("Hook target missing — {task}"),"body":body,"locale":requested_locale,"relativePath":format!("/{requested_locale}"),"dedupeKey":format!("hook-missing:{task}:{status}:{}",text(payload,"reason"))}),
            )
        }
        "background-sync-review-needed" => {
            let title = match lang {
                "en" => "Background sync review needed",
                "zh" => "需要检查后台同步结果",
                _ => "백그라운드 sync 검토 필요",
            };
            let mut lines = Vec::new();
            let pulls = list(payload, "pulledTasks");
            let counts = [
                list(payload, "mergedPullRequests").len(),
                list(payload, "registeredWorktrees").len(),
                pulls.iter().filter(|v| v["status"] == "updated").count(),
                pulls.iter().filter(|v| v["status"] == "failed").count(),
                list(payload, "failures").len(),
            ];
            for (index, count) in counts.into_iter().enumerate() {
                if count == 0 {
                    continue;
                }
                lines.push(match lang {
                    "en" => format!(
                        "{count} {}{}",
                        [
                            "merged PR",
                            "new TODO worktree",
                            "pull update",
                            "failed pull",
                            "sync failure"
                        ][index],
                        if count == 1 { "" } else { "s" }
                    ),
                    "zh" => format!(
                        "{} {count} 个",
                        [
                            "已合并 PR",
                            "新建 TODO worktree",
                            "pull 完成",
                            "pull 失败",
                            "sync 失败"
                        ][index]
                    ),
                    _ => format!(
                        "{} {count}건",
                        [
                            "merge된 PR",
                            "새 TODO worktree",
                            "pull 완료",
                            "pull 실패",
                            "sync 실패"
                        ][index]
                    ),
                });
            }
            let mut dedupe = format!(
                "background-sync-review:{}::{}::{}",
                keys(
                    payload,
                    "mergedPullRequests",
                    &["taskId", "prUrl", "mergedAt"]
                ),
                keys(
                    payload,
                    "registeredWorktrees",
                    &["taskId", "branchName", "worktreePath"]
                ),
                keys(
                    payload,
                    "pulledTasks",
                    &["taskId", "branchName", "status", "summary"]
                )
            );
            let mut failures: Vec<_> = list(payload, "failures")
                .iter()
                .map(|v| {
                    format!(
                        "{}:{}:{}:{}",
                        text(v, "operation"),
                        v["taskId"].as_str().unwrap_or_else(|| text(v, "target")),
                        text(v, "branchName"),
                        text(v, "reason")
                    )
                })
                .collect();
            failures.sort();
            if !failures.is_empty() {
                dedupe.push_str("::");
                dedupe.push_str(&failures.join("|"));
            }
            let mut review = json!({"mergedPullRequests":list(payload,"mergedPullRequests"),"registeredWorktrees":list(payload,"registeredWorktrees"),"pulledTasks":pulls});
            if !list(payload, "failures").is_empty() {
                review["failures"] = json!(list(payload, "failures"));
            }
            Some(
                json!({"title":title,"body":lines.join(" / "),"locale":requested_locale,"relativePath":format!("/{requested_locale}"),"dedupeKey":dedupe,"action":{"type":"background-sync-review","payload":review}}),
            )
        }
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_notification_matches_renderer_dedupe_and_locale() {
        let p=prepare(&json!({"type":"task-status-changed","taskId":"t","projectName":"P","branchName":"main","taskTitle":"T","newStatus":"review","description":"D"}),"ko").unwrap();
        assert_eq!(p["body"], "T: review로 변경\nD");
        assert_eq!(p["dedupeKey"], "task-status:t:review");
    }
    #[test]
    fn background_review_retains_actions_and_failure_dedupe() {
        let p=prepare(&json!({"type":"background-sync-review-needed","mergedPullRequests":[],"registeredWorktrees":[],"pulledTasks":[{"taskId":"t","branchName":"b","status":"updated","summary":"s"}],"failures":[{"operation":"pull","target":"P","reason":"failed"}]}),"en").unwrap();
        assert_eq!(p["body"], "1 pull update / 1 sync failure");
        assert_eq!(p["action"]["type"], "background-sync-review");
        assert!(text(&p, "dedupeKey").ends_with("::pull:P::failed"));
    }
}
