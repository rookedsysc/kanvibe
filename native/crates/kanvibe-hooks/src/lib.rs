use std::collections::BTreeMap;

pub const QA_SOCKET_ENV: &str = "KANVIBE_QA_SOCKET";
pub const HOOK_SERVER_PORT_ENV: &str = "KANVIBE_HOOK_SERVER_PORT";
pub const DEFAULT_HOOK_SERVER_PORT: u16 = 9736;
pub const DEV_HOOK_SERVER_PORT: u16 = 19736;
pub const AI_PROVIDERS: &[AiSessionProvider] = &[
    AiSessionProvider::Claude,
    AiSessionProvider::Codex,
    AiSessionProvider::Gemini,
    AiSessionProvider::OpenCode,
];

pub fn is_kanvibe_scoped_env(key: &str) -> bool {
    key == QA_SOCKET_ENV || key == HOOK_SERVER_PORT_ENV || key.starts_with("KANVIBE_")
}

pub fn local_hook_server_url(port: u16) -> String {
    format!("http://localhost:{port}")
}

pub fn remote_hook_server_url(host: &str, port: u16) -> String {
    format!("http://{host}:{port}")
}

pub fn extract_shell_hook_server_url(content: &str) -> Option<String> {
    content
        .lines()
        .find_map(|line| line.strip_prefix("KANVIBE_URL=\""))
        .and_then(|value| value.strip_suffix('"'))
        .map(ToOwned::to_owned)
}

pub fn extract_plugin_hook_server_url(content: &str) -> Option<String> {
    content
        .split("const KANVIBE_URL = \"")
        .nth(1)
        .and_then(|tail| tail.split('"').next())
        .map(ToOwned::to_owned)
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HookServerValidation {
    pub has_expected_hook_server_url: bool,
    pub has_reachable_hook_server: bool,
    pub expected_hook_server_url: Option<String>,
    pub configured_hook_server_url: Option<String>,
}

pub fn validate_hook_server_configuration(
    configured_urls: &[Option<String>],
    expected_url: Option<&str>,
    should_validate: bool,
    is_remote: bool,
    reachable: bool,
) -> HookServerValidation {
    let defined_urls = configured_urls
        .iter()
        .filter_map(|url| url.as_deref())
        .filter(|url| !url.is_empty())
        .collect::<Vec<_>>();
    let configured_hook_server_url = defined_urls.first().map(|url| (*url).to_owned());

    if !should_validate {
        return HookServerValidation {
            has_expected_hook_server_url: true,
            has_reachable_hook_server: true,
            expected_hook_server_url: None,
            configured_hook_server_url,
        };
    }

    let Some(expected_url) = expected_url else {
        return HookServerValidation {
            has_expected_hook_server_url: false,
            has_reachable_hook_server: false,
            expected_hook_server_url: None,
            configured_hook_server_url,
        };
    };

    let has_expected_hook_server_url = defined_urls.len() == configured_urls.len()
        && !defined_urls.is_empty()
        && defined_urls
            .iter()
            .all(|configured| is_expected_hook_server_url(configured, expected_url, is_remote));

    HookServerValidation {
        has_expected_hook_server_url,
        has_reachable_hook_server: has_expected_hook_server_url && reachable,
        expected_hook_server_url: Some(expected_url.to_owned()),
        configured_hook_server_url,
    }
}

fn is_expected_hook_server_url(configured: &str, expected: &str, is_remote: bool) -> bool {
    let Some(configured) = ParsedUrl::parse(configured) else {
        return false;
    };
    let Some(expected) = ParsedUrl::parse(expected) else {
        return false;
    };

    configured.protocol == expected.protocol
        && configured.port == expected.port
        && (is_remote || normalize_loopback(configured.host) == normalize_loopback(expected.host))
}

fn normalize_loopback(host: &str) -> &str {
    if host == "127.0.0.1" {
        "localhost"
    } else {
        host
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct ParsedUrl<'a> {
    protocol: &'a str,
    host: &'a str,
    port: &'a str,
}

impl<'a> ParsedUrl<'a> {
    fn parse(url: &'a str) -> Option<Self> {
        let (protocol, rest) = url.split_once("://")?;
        let authority = rest.split('/').next().unwrap_or(rest);
        let (host, port) = authority.rsplit_once(':')?;
        Some(Self {
            protocol,
            host,
            port,
        })
    }
}

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub enum AiSessionProvider {
    Claude,
    Codex,
    Gemini,
    OpenCode,
}

impl AiSessionProvider {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::OpenCode => "opencode",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AiSession {
    pub id: String,
    pub provider: AiSessionProvider,
    pub updated_at: String,
    pub matched_path: Option<String>,
    pub title: Option<String>,
    pub message_count: u32,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AiSourceStatus {
    pub provider: AiSessionProvider,
    pub available: bool,
    pub session_count: usize,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AiSessionAggregation {
    pub sessions: Vec<AiSession>,
    pub sources: Vec<AiSourceStatus>,
}

pub fn aggregate_ai_sessions(sessions: Vec<AiSession>) -> AiSessionAggregation {
    let mut sessions = sessions;
    sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));

    let counts = sessions
        .iter()
        .fold(BTreeMap::new(), |mut counts, session| {
            *counts.entry(session.provider).or_insert(0usize) += 1;
            counts
        });
    let sources = AI_PROVIDERS
        .iter()
        .map(|provider| AiSourceStatus {
            provider: *provider,
            available: true,
            session_count: *counts.get(provider).unwrap_or(&0),
            reason: None,
        })
        .collect();

    AiSessionAggregation { sessions, sources }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum BoardEvent {
    BoardUpdated,
    TaskStatusChanged {
        task_id: String,
        task_title: String,
        new_status: String,
    },
    TaskHookInstallFailed {
        task_id: String,
        task_title: String,
        error: String,
    },
    BackgroundSyncReviewNeeded {
        merged_pull_request_count: usize,
        registered_worktree_count: usize,
        failure_count: usize,
    },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AppNotification {
    pub id: String,
    pub title: String,
    pub body: String,
    pub read: bool,
}

#[derive(Debug, Default)]
pub struct NotificationCenter {
    notifications: Vec<AppNotification>,
}

impl NotificationCenter {
    pub fn push_event(&mut self, event: BoardEvent) -> Option<&AppNotification> {
        let notification = notification_from_board_event(event)?;
        self.notifications.push(notification);
        self.notifications.last()
    }

    pub fn unread_count(&self) -> usize {
        self.notifications
            .iter()
            .filter(|notification| !notification.read)
            .count()
    }

    pub fn list(&self) -> &[AppNotification] {
        &self.notifications
    }

    pub fn mark_all_read(&mut self) {
        for notification in &mut self.notifications {
            notification.read = true;
        }
    }
}

pub fn notification_from_board_event(event: BoardEvent) -> Option<AppNotification> {
    match event {
        BoardEvent::BoardUpdated => None,
        BoardEvent::TaskStatusChanged {
            task_id,
            task_title,
            new_status,
        } => Some(AppNotification {
            id: format!("task-status:{task_id}:{new_status}"),
            title: "Task status changed".to_owned(),
            body: format!("{task_title} moved to {new_status}"),
            read: false,
        }),
        BoardEvent::TaskHookInstallFailed {
            task_id,
            task_title,
            error,
        } => Some(AppNotification {
            id: format!("hook-install-failed:{task_id}"),
            title: "Hook install failed".to_owned(),
            body: format!("{task_title}: {error}"),
            read: false,
        }),
        BoardEvent::BackgroundSyncReviewNeeded {
            merged_pull_request_count,
            registered_worktree_count,
            failure_count,
        } => Some(AppNotification {
            id: "background-sync-review-needed".to_owned(),
            title: "Background sync needs review".to_owned(),
            body: format!(
                "{merged_pull_request_count} merged PRs, {registered_worktree_count} worktrees, {failure_count} failures"
            ),
            read: false,
        }),
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HookProviderStatus {
    pub provider: AiSessionProvider,
    pub installed: bool,
    pub has_expected_hook_server_url: bool,
    pub has_reachable_hook_server: bool,
}

pub fn hook_status_visible(statuses: &[HookProviderStatus]) -> bool {
    AI_PROVIDERS
        .iter()
        .all(|provider| statuses.iter().any(|status| status.provider == *provider))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_env_contract_is_kanvibe_scoped() {
        assert!(is_kanvibe_scoped_env(QA_SOCKET_ENV));
        assert!(is_kanvibe_scoped_env(HOOK_SERVER_PORT_ENV));
        assert!(!is_kanvibe_scoped_env("PORT"));
    }

    #[test]
    fn hook_server_url_validation_matches_local_and_remote_rules() {
        let local = validate_hook_server_configuration(
            &[Some("http://127.0.0.1:9736".to_owned())],
            Some("http://localhost:9736"),
            true,
            false,
            true,
        );
        assert!(local.has_expected_hook_server_url);
        assert!(local.has_reachable_hook_server);

        let remote = validate_hook_server_configuration(
            &[Some("http://100.83.96.29:9736".to_owned())],
            Some("http://10.0.0.42:9736"),
            true,
            true,
            true,
        );
        assert!(remote.has_expected_hook_server_url);
    }

    #[test]
    fn hook_url_extractors_read_shell_and_plugin_content() {
        assert_eq!(
            extract_shell_hook_server_url("KANVIBE_URL=\"http://localhost:9736\"\n"),
            Some("http://localhost:9736".to_owned())
        );
        assert_eq!(
            extract_plugin_hook_server_url("const KANVIBE_URL = \"http://localhost:9736\";"),
            Some("http://localhost:9736".to_owned())
        );
    }

    #[test]
    fn ai_session_aggregation_sorts_and_reports_provider_sources() {
        let result = aggregate_ai_sessions(vec![
            AiSession {
                id: "old".to_owned(),
                provider: AiSessionProvider::Claude,
                updated_at: "2026-07-08T00:00:00Z".to_owned(),
                matched_path: None,
                title: None,
                message_count: 2,
            },
            AiSession {
                id: "new".to_owned(),
                provider: AiSessionProvider::Codex,
                updated_at: "2026-07-08T01:00:00Z".to_owned(),
                matched_path: None,
                title: None,
                message_count: 4,
            },
        ]);

        assert_eq!(result.sessions[0].id, "new");
        assert_eq!(result.sources.len(), 4);
        assert_eq!(result.sources[0].provider, AiSessionProvider::Claude);
        assert_eq!(result.sources[0].session_count, 1);
    }

    #[test]
    fn notification_center_converts_board_events_and_marks_read() {
        let mut center = NotificationCenter::default();
        center.push_event(BoardEvent::TaskHookInstallFailed {
            task_id: "task-1".to_owned(),
            task_title: "Install hooks".to_owned(),
            error: "missing config".to_owned(),
        });

        assert_eq!(center.unread_count(), 1);
        assert_eq!(center.list()[0].title, "Hook install failed");
        center.mark_all_read();
        assert_eq!(center.unread_count(), 0);
    }

    #[test]
    fn hook_status_requires_all_ai_providers() {
        let statuses = AI_PROVIDERS
            .iter()
            .map(|provider| HookProviderStatus {
                provider: *provider,
                installed: true,
                has_expected_hook_server_url: true,
                has_reachable_hook_server: true,
            })
            .collect::<Vec<_>>();

        assert!(hook_status_visible(&statuses));
    }
}
