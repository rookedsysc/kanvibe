//! 작업 상태 변경, hook 대상 누락, hook 터널 실패·복구, 백그라운드 sync 검토 알림 문구. 메인 알림과 모바일 알림이 같은 문구를 쓴다.

use crate::i18n::Locale;
use crate::notifications::{
    AppNotificationAction, BackgroundSyncFailurePayload, BackgroundSyncPulledTaskPayload,
    BackgroundSyncReviewPayload, DesktopNotificationPayload, HookTunnelFailedPayload,
    HookTunnelFailureKind, HookTunnelRecoveredPayload, PulledTaskStatus,
};

/// 알림 locale은 `en-US`처럼 지역이 붙어 올 수 있어 앞부분만 본다. 모르는 값은 한국어로 보낸다.
pub fn notification_locale(locale: &str) -> Locale {
    if locale.starts_with("en") {
        Locale::En
    } else if locale.starts_with("zh") {
        Locale::Zh
    } else {
        Locale::Ko
    }
}

fn plural_suffix(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn format_status_changed(locale: Locale, task_title: &str, new_status: &str) -> String {
    match locale {
        Locale::Ko => format!("{task_title}: {new_status}로 변경"),
        Locale::En => format!("{task_title}: changed to {new_status}"),
        Locale::Zh => format!("{task_title}: 已变更为{new_status}"),
    }
}

fn format_missing_status(locale: Locale, requested_status: &str) -> String {
    match locale {
        Locale::Ko => format!("{requested_status} 상태로 변경하지 못했습니다."),
        Locale::En => format!("Failed to change status to {requested_status}."),
        Locale::Zh => format!("未能变更为 {requested_status} 状态。"),
    }
}

fn task_not_found_message(locale: Locale) -> &'static str {
    match locale {
        Locale::Ko => "연결된 작업을 찾지 못했습니다.",
        Locale::En => "No matching task was found.",
        Locale::Zh => "未找到匹配的任务。",
    }
}

/// 실패 갈래의 제목과 확인할 곳
fn hook_tunnel_failure_message(
    locale: Locale,
    kind: HookTunnelFailureKind,
) -> (&'static str, &'static str) {
    use HookTunnelFailureKind::*;
    match (locale, kind) {
        (Locale::Ko, Ssh) => (
            "SSH 연결 실패",
            "네트워크, VPN, 호스트 전원 및 SSH 설정을 확인하세요.",
        ),
        (Locale::Ko, Handshake) => (
            "KanVibe 핸드셰이크 실패",
            "원격 daemon과 데스크톱의 KanVibe 버전 및 설정을 확인하세요.",
        ),
        (Locale::Ko, ClientInUse) => (
            "KanVibe 연결 거부",
            "같은 클라이언트 ID의 기존 연결이 응답했습니다. 기존 데스크톱의 연결을 종료한 뒤 다시 시도하세요.",
        ),
        (Locale::Ko, Setup) => (
            "Hook 터널 준비 실패",
            "원격 호스트의 KanVibe 실행 환경을 확인하세요.",
        ),
        (Locale::En, Ssh) => (
            "SSH connection failed",
            "Check the network, VPN, host power, and SSH configuration.",
        ),
        (Locale::En, Handshake) => (
            "KanVibe handshake failed",
            "Check the KanVibe versions and configuration on the remote host and desktop.",
        ),
        (Locale::En, ClientInUse) => (
            "KanVibe connection rejected",
            "An existing connection with this client ID responded. Disconnect the other desktop and try again.",
        ),
        (Locale::En, Setup) => (
            "Hook tunnel setup failed",
            "Check the KanVibe runtime on the remote host.",
        ),
        (Locale::Zh, Ssh) => ("SSH 连接失败", "请检查网络、VPN、主机电源和 SSH 配置。"),
        (Locale::Zh, Handshake) => (
            "KanVibe 握手失败",
            "请检查远程主机和桌面端的 KanVibe 版本和配置。",
        ),
        (Locale::Zh, ClientInUse) => (
            "KanVibe 连接被拒绝",
            "使用相同客户端 ID 的现有连接已响应。请断开另一台桌面端的连接后重试。",
        ),
        (Locale::Zh, Setup) => (
            "Hook 隧道初始化失败",
            "请检查远程主机上的 KanVibe 运行环境。",
        ),
    }
}

/// 자동 복구 알림의 제목과 본문
fn hook_tunnel_recovered_message(locale: Locale) -> (&'static str, &'static str) {
    match locale {
        Locale::Ko => (
            "Hook 터널 복구",
            "같은 클라이언트 ID의 기존 연결이 남아 있었습니다. 응답이 없어 기존 연결을 교체했습니다.",
        ),
        Locale::En => (
            "Hook tunnel recovered",
            "An existing connection with this client ID did not respond and was replaced.",
        ),
        Locale::Zh => (
            "Hook 隧道已恢复",
            "使用相同客户端 ID 的现有连接没有响应，已将其替换。",
        ),
    }
}

fn background_sync_review_title(locale: Locale) -> &'static str {
    match locale {
        Locale::Ko => "백그라운드 sync 검토 필요",
        Locale::En => "Background sync review needed",
        Locale::Zh => "需要检查后台同步结果",
    }
}

/// 백그라운드 sync 요약 줄의 종류
#[derive(Debug, Clone, Copy)]
enum SyncSummaryKind {
    MergedPullRequests,
    RegisteredWorktrees,
    UpdatedPulls,
    FailedPulls,
    SyncFailures,
}

fn format_sync_summary(locale: Locale, kind: SyncSummaryKind, count: usize) -> String {
    use SyncSummaryKind::*;
    let suffix = plural_suffix(count);
    match (locale, kind) {
        (Locale::Ko, MergedPullRequests) => format!("merge된 PR {count}건"),
        (Locale::Ko, RegisteredWorktrees) => format!("새 TODO worktree {count}건"),
        (Locale::Ko, UpdatedPulls) => format!("pull 완료 {count}건"),
        (Locale::Ko, FailedPulls) => format!("pull 실패 {count}건"),
        (Locale::Ko, SyncFailures) => format!("sync 실패 {count}건"),
        (Locale::En, MergedPullRequests) => format!("{count} merged PR{suffix}"),
        (Locale::En, RegisteredWorktrees) => format!("{count} new TODO worktree{suffix}"),
        (Locale::En, UpdatedPulls) => format!("{count} pull update{suffix}"),
        (Locale::En, FailedPulls) => format!("{count} failed pull{suffix}"),
        (Locale::En, SyncFailures) => format!("{count} sync failure{suffix}"),
        (Locale::Zh, MergedPullRequests) => format!("已合并 PR {count} 个"),
        (Locale::Zh, RegisteredWorktrees) => format!("新建 TODO worktree {count} 个"),
        (Locale::Zh, UpdatedPulls) => format!("pull 完成 {count} 个"),
        (Locale::Zh, FailedPulls) => format!("pull 失败 {count} 个"),
        (Locale::Zh, SyncFailures) => format!("sync 失败 {count} 个"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStatusNotification {
    pub project_name: String,
    pub branch_name: String,
    pub task_title: String,
    pub description: Option<String>,
    pub new_status: String,
    pub task_id: String,
    pub locale: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookStatusTargetMissingNotification {
    pub task_id: String,
    pub requested_status: String,
    pub locale: String,
}

/// 데스크톱 알림 제목·본문과 알림 목록에 남길 내용
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedNotification {
    pub title: String,
    pub body: String,
    pub desktop_payload: DesktopNotificationPayload,
}

pub fn build_task_status_notification(payload: &TaskStatusNotification) -> PreparedNotification {
    let locale = notification_locale(&payload.locale);
    let title = format!("{} — {}", payload.project_name, payload.branch_name);
    let mut body_lines = vec![format_status_changed(
        locale,
        &payload.task_title,
        &payload.new_status,
    )];
    if let Some(description) = payload
        .description
        .as_deref()
        .filter(|text| !text.is_empty())
    {
        body_lines.push(description.to_string());
    }
    let body = body_lines.join("\n");

    PreparedNotification {
        desktop_payload: DesktopNotificationPayload {
            title: title.clone(),
            body: body.clone(),
            task_id: Some(payload.task_id.clone()),
            locale: payload.locale.clone(),
            relative_path: Some(format!("/{}/task/{}", payload.locale, payload.task_id)),
            dedupe_key: Some(format!(
                "task-status:{}:{}",
                payload.task_id, payload.new_status
            )),
            action: None,
        },
        title,
        body,
    }
}

/// hook이 보낸 작업 ID에 해당하는 작업이 없을 때의 알림. 사유는 지금 `task-not-found` 하나뿐이다
pub fn build_hook_status_target_missing_notification(
    payload: &HookStatusTargetMissingNotification,
) -> PreparedNotification {
    let locale = notification_locale(&payload.locale);
    let title = format!("Hook target missing — {}", payload.task_id);
    let body = format!(
        "{}\n{}",
        format_missing_status(locale, &payload.requested_status),
        task_not_found_message(locale)
    );

    PreparedNotification {
        desktop_payload: DesktopNotificationPayload {
            title: title.clone(),
            body: body.clone(),
            task_id: None,
            locale: payload.locale.clone(),
            relative_path: Some(format!("/{}", payload.locale)),
            dedupe_key: Some(format!(
                "hook-missing:{}:{}:task-not-found",
                payload.task_id, payload.requested_status
            )),
            action: None,
        },
        title,
        body,
    }
}

/// hook 터널 실패 알림. 본문 첫 줄은 확인할 곳, 다음 줄은 실패 사유 원문이다
pub fn build_hook_tunnel_failed_notification(
    payload: &HookTunnelFailedPayload,
    locale_text: &str,
) -> DesktopNotificationPayload {
    let (title, hint) = hook_tunnel_failure_message(notification_locale(locale_text), payload.kind);
    build_hook_tunnel_notification(
        &payload.ssh_host,
        title,
        format!("{hint}\n{}", payload.reason),
        locale_text,
    )
}

/// 응답 없는 기존 연결을 교체해 hook 터널이 다시 이어졌다는 알림
pub fn build_hook_tunnel_recovered_notification(
    payload: &HookTunnelRecoveredPayload,
    locale_text: &str,
) -> DesktopNotificationPayload {
    let (title, body) = hook_tunnel_recovered_message(notification_locale(locale_text));
    build_hook_tunnel_notification(&payload.ssh_host, title, body.to_string(), locale_text)
}

fn build_hook_tunnel_notification(
    ssh_host: &str,
    title: &str,
    body: String,
    locale_text: &str,
) -> DesktopNotificationPayload {
    DesktopNotificationPayload {
        title: format!("{ssh_host} — {title}"),
        body,
        task_id: None,
        locale: locale_text.to_string(),
        relative_path: None,
        dedupe_key: None,
        action: None,
    }
}

fn summarize_background_sync(
    locale: Locale,
    review: &BackgroundSyncReviewPayload,
    pulled_tasks: &[BackgroundSyncPulledTaskPayload],
    failures: &[BackgroundSyncFailurePayload],
) -> String {
    let count_pulls = |status| {
        pulled_tasks
            .iter()
            .filter(|task| task.status == status)
            .count()
    };
    let summary_counts = [
        (
            SyncSummaryKind::MergedPullRequests,
            review.merged_pull_requests.len(),
        ),
        (
            SyncSummaryKind::RegisteredWorktrees,
            review.registered_worktrees.len(),
        ),
        (
            SyncSummaryKind::UpdatedPulls,
            count_pulls(PulledTaskStatus::Updated),
        ),
        (
            SyncSummaryKind::FailedPulls,
            count_pulls(PulledTaskStatus::Failed),
        ),
        (SyncSummaryKind::SyncFailures, failures.len()),
    ];
    let summary_lines: Vec<String> = summary_counts
        .into_iter()
        .filter(|&(_, count)| count > 0)
        .map(|(kind, count)| format_sync_summary(locale, kind, count))
        .collect();
    summary_lines.join(" / ")
}

/// 항목 순서가 달라도 같은 검토 결과면 같은 키가 나오도록 정렬해 잇는다
fn sorted_join(keys: impl Iterator<Item = String>) -> String {
    let mut sorted_keys: Vec<String> = keys.collect();
    sorted_keys.sort();
    sorted_keys.join("|")
}

fn background_sync_dedupe_key(
    review: &BackgroundSyncReviewPayload,
    pulled_tasks: &[BackgroundSyncPulledTaskPayload],
    failures: &[BackgroundSyncFailurePayload],
) -> String {
    let merged_keys = sorted_join(
        review
            .merged_pull_requests
            .iter()
            .map(|item| format!("{}:{}:{}", item.task_id, item.pr_url, item.merged_at)),
    );
    let worktree_keys = sorted_join(review.registered_worktrees.iter().map(|item| {
        format!(
            "{}:{}:{}",
            item.task_id, item.branch_name, item.worktree_path
        )
    }));
    let pull_keys = sorted_join(pulled_tasks.iter().map(|item| {
        let status = match item.status {
            PulledTaskStatus::Updated => "updated",
            PulledTaskStatus::Failed => "failed",
        };
        format!(
            "{}:{}:{}:{}",
            item.task_id, item.branch_name, status, item.summary
        )
    }));
    let failure_keys = sorted_join(failures.iter().map(|item| {
        let target = item.task_id.as_deref().unwrap_or(&item.target);
        let branch_name = item.branch_name.as_deref().unwrap_or_default();
        format!(
            "{}:{target}:{branch_name}:{}",
            item.operation.as_str(),
            item.reason
        )
    }));

    let dedupe_key = format!("background-sync-review:{merged_keys}::{worktree_keys}::{pull_keys}");
    if failure_keys.is_empty() {
        dedupe_key
    } else {
        format!("{dedupe_key}::{failure_keys}")
    }
}

/// 본문에는 건수 요약만 넣고, 항목 상세는 알림을 눌렀을 때 열 검토 화면이 쓰도록 action에 싣는다
pub fn build_background_sync_review_notification(
    review: &BackgroundSyncReviewPayload,
    locale_text: &str,
) -> PreparedNotification {
    let locale = notification_locale(locale_text);
    let pulled_tasks = review.pulled_tasks.clone().unwrap_or_default();
    let failures = review.failures.clone().unwrap_or_default();
    let title = background_sync_review_title(locale).to_string();
    let body = summarize_background_sync(locale, review, &pulled_tasks, &failures);
    let dedupe_key = background_sync_dedupe_key(review, &pulled_tasks, &failures);
    let action_payload = BackgroundSyncReviewPayload {
        merged_pull_requests: review.merged_pull_requests.clone(),
        registered_worktrees: review.registered_worktrees.clone(),
        pulled_tasks: Some(pulled_tasks),
        failures: (!failures.is_empty()).then_some(failures),
    };

    PreparedNotification {
        desktop_payload: DesktopNotificationPayload {
            title: title.clone(),
            body: body.clone(),
            task_id: None,
            locale: locale_text.to_string(),
            relative_path: Some(format!("/{locale_text}")),
            dedupe_key: Some(dedupe_key),
            action: Some(AppNotificationAction::BackgroundSyncReview {
                payload: action_payload,
            }),
        },
        title,
        body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::BackgroundSyncOperation;

    // parity: TS-shared-taskNotifications-1ef320
    #[test]
    fn unknown_locale_falls_back_to_korean() {
        assert_eq!(notification_locale("fr-FR"), Locale::Ko);
        assert_eq!(notification_locale("en-US"), Locale::En);
        assert_eq!(notification_locale("zh-CN"), Locale::Zh);
    }

    // parity: TS-shared-taskNotifications-52363d
    #[test]
    fn task_status_notification_has_a_stable_path_and_dedupe_key() {
        let notification = build_task_status_notification(&TaskStatusNotification {
            project_name: "kanvibe".into(),
            branch_name: "fix/session-prompt".into(),
            task_title: "세션 프롬프트 회귀".into(),
            description: Some("원격 sshHost 전달 누락 수정".into()),
            new_status: "review".into(),
            task_id: "task-1".into(),
            locale: "fr-FR".into(),
        });

        assert_eq!(notification.title, "kanvibe — fix/session-prompt");
        assert_eq!(
            notification.body,
            "세션 프롬프트 회귀: review로 변경\n원격 sshHost 전달 누락 수정"
        );
        assert_eq!(
            serde_json::to_string(&notification.desktop_payload).unwrap(),
            r#"{"title":"kanvibe — fix/session-prompt","body":"세션 프롬프트 회귀: review로 변경\n원격 sshHost 전달 누락 수정","taskId":"task-1","locale":"fr-FR","relativePath":"/fr-FR/task/task-1","dedupeKey":"task-status:task-1:review"}"#
        );
    }

    // parity: TS-shared-taskNotifications-4bd230
    #[test]
    fn missing_target_notification_uses_the_locale_message_and_root_path() {
        let notification =
            build_hook_status_target_missing_notification(&HookStatusTargetMissingNotification {
                task_id: "task-404".into(),
                requested_status: "pending".into(),
                locale: "zh-CN".into(),
            });

        assert_eq!(notification.title, "Hook target missing — task-404");
        assert_eq!(
            notification.body,
            "未能变更为 pending 状态。\n未找到匹配的任务。"
        );
        assert_eq!(
            serde_json::to_string(&notification.desktop_payload).unwrap(),
            r#"{"title":"Hook target missing — task-404","body":"未能变更为 pending 状态。\n未找到匹配的任务。","locale":"zh-CN","relativePath":"/zh-CN","dedupeKey":"hook-missing:task-404:pending:task-not-found"}"#
        );
    }

    fn pulled_task(
        task_id: &str,
        branch_name: &str,
        status: PulledTaskStatus,
        summary: &str,
    ) -> BackgroundSyncPulledTaskPayload {
        BackgroundSyncPulledTaskPayload {
            task_id: task_id.into(),
            task_title: "Pull".into(),
            branch_name: branch_name.into(),
            worktree_path: format!("/workspace/repo__worktrees/{task_id}"),
            ssh_host: None,
            status,
            summary: summary.into(),
        }
    }

    // parity: TS-shared-taskNotifications-79d9db
    #[test]
    fn background_sync_body_summarizes_and_the_action_carries_details() {
        let pulled_tasks = vec![
            pulled_task(
                "task-pull-a",
                "feature/pull-a",
                PulledTaskStatus::Updated,
                "Fast-forward",
            ),
            pulled_task(
                "task-pull-b",
                "feature/pull-b",
                PulledTaskStatus::Failed,
                "Not possible to fast-forward",
            ),
        ];
        let failures = vec![BackgroundSyncFailurePayload {
            operation: BackgroundSyncOperation::PullRequestSync,
            target: "PR sync target (feature/pr-fail)".into(),
            reason: "gh auth failed".into(),
            task_id: Some("task-11".into()),
            branch_name: Some("feature/pr-fail".into()),
            ssh_host: None,
        }];
        let review = BackgroundSyncReviewPayload {
            merged_pull_requests: vec![],
            registered_worktrees: vec![],
            pulled_tasks: Some(pulled_tasks.clone()),
            failures: Some(failures.clone()),
        };

        let notification = build_background_sync_review_notification(&review, "ko");

        assert_eq!(
            notification.body,
            "pull 완료 1건 / pull 실패 1건 / sync 실패 1건"
        );
        assert_eq!(
            notification.desktop_payload.dedupe_key.as_deref(),
            Some(
                "background-sync-review:::::task-pull-a:feature/pull-a:updated:Fast-forward|task-pull-b:feature/pull-b:failed:Not possible to fast-forward::pull-request-sync:task-11:feature/pr-fail:gh auth failed"
            )
        );
        assert_eq!(
            notification.desktop_payload.action,
            Some(AppNotificationAction::BackgroundSyncReview { payload: review })
        );
    }

    #[test]
    fn background_sync_action_always_carries_pulled_tasks_but_only_present_failures() {
        let review = BackgroundSyncReviewPayload {
            merged_pull_requests: vec![],
            registered_worktrees: vec![],
            pulled_tasks: None,
            failures: Some(vec![]),
        };

        let notification = build_background_sync_review_notification(&review, "en");
        let action_json = serde_json::to_string(&notification.desktop_payload.action).unwrap();

        assert_eq!(
            action_json,
            r#"{"type":"background-sync-review","payload":{"mergedPullRequests":[],"registeredWorktrees":[],"pulledTasks":[]}}"#
        );
        assert_eq!(notification.title, "Background sync review needed");
        assert_eq!(notification.body, "");
    }
}
