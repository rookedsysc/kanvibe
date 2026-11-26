//! 등록된 모든 계정의 사용량을 한 번에 모은다. 계정 하나의 실패가 나머지를 가리지 않도록 결과를 따로 받고
//! 실패한 자리에만 error를 채운다.

use crate::error::AiResult;
use crate::session_listing::map_with_concurrency;
use crate::usage::account_discovery::discover_provider_accounts;
use crate::usage::account_registry::AiAccountRegistration;
use crate::usage::claude_usage::read_claude_usage;
use crate::usage::codex_usage::read_codex_usage;
use crate::usage::context::UsageContext;
use crate::usage::gemini_usage::read_gemini_usage;
use crate::usage::shared::{create_error_usage, create_unavailable_usage};
use crate::usage::types::{
    AiUsageAccount, AiUsageAccountResult, AiUsageFailureReason, AiUsageProvider, AiUsageSnapshot,
};

/// 계정 탐색과 계정별 조회. 집계 규칙을 원천과 떼어 시험할 수 있게 둔다
pub trait UsageProviderSources: Send + Sync {
    fn discover_accounts(
        &self,
        provider: AiUsageProvider,
        registrations: &[AiAccountRegistration],
    ) -> AiResult<Vec<AiUsageAccount>>;
    fn read_usage(&self, account: &AiUsageAccount) -> AiResult<AiUsageAccountResult>;
    fn now_iso(&self) -> String;
}

impl UsageProviderSources for UsageContext {
    fn discover_accounts(
        &self,
        provider: AiUsageProvider,
        registrations: &[AiAccountRegistration],
    ) -> AiResult<Vec<AiUsageAccount>> {
        Ok(discover_provider_accounts(self, provider, registrations))
    }

    fn read_usage(&self, account: &AiUsageAccount) -> AiResult<AiUsageAccountResult> {
        Ok(match account.provider {
            AiUsageProvider::Claude => read_claude_usage(self, account),
            AiUsageProvider::Codex => read_codex_usage(self, account),
            AiUsageProvider::Gemini => read_gemini_usage(self, account),
        })
    }

    fn now_iso(&self) -> String {
        UsageContext::now_iso(self)
    }
}

/// 로그인한 계정이 없는 provider를 대신할 자리표시자. 카드까지 사라지면 "지원하지 않는다"와 "로그인하면 보인다"를 구분할 수 없다
fn create_signed_out_account(provider: AiUsageProvider) -> AiUsageAccount {
    AiUsageAccount {
        provider,
        account_id: provider.id().to_string(),
        label: provider.id().to_string(),
        config_dir: String::new(),
        account_root: String::new(),
    }
}

/// 조회할 계정과 그 계정이 로그아웃 자리표시자인지
struct AccountUsageTask {
    account: AiUsageAccount,
    is_signed_out_placeholder: bool,
}

fn build_usage_tasks(
    provider: AiUsageProvider,
    accounts: Vec<AiUsageAccount>,
) -> Vec<AccountUsageTask> {
    if accounts.is_empty() {
        return vec![AccountUsageTask {
            account: create_signed_out_account(provider),
            is_signed_out_placeholder: true,
        }];
    }
    accounts
        .into_iter()
        .map(|account| AccountUsageTask {
            account,
            is_signed_out_placeholder: false,
        })
        .collect()
}

fn read_task(sources: &dyn UsageProviderSources, task: &AccountUsageTask) -> AiUsageAccountResult {
    if task.is_signed_out_placeholder {
        return create_unavailable_usage(
            &task.account,
            AiUsageFailureReason::MissingCredentials,
            sources.now_iso(),
        );
    }
    sources.read_usage(&task.account).unwrap_or_else(|_| {
        create_error_usage(
            &task.account,
            AiUsageFailureReason::FetchFailed,
            sources.now_iso(),
        )
    })
}

/// 화면 순서는 provider 순서가 정한다. 한 provider의 탐색 실패는 빈 자리로 두고 나머지 provider를 조회한다
pub fn aggregate_ai_usage(
    sources: &dyn UsageProviderSources,
    registrations: &[AiAccountRegistration],
) -> AiUsageSnapshot {
    let providers = AiUsageProvider::ALL;
    let discovered: Vec<Vec<AiUsageAccount>> =
        map_with_concurrency(&providers, providers.len(), |provider, _| {
            sources
                .discover_accounts(*provider, registrations)
                .unwrap_or_default()
        });
    let tasks: Vec<AccountUsageTask> = providers
        .into_iter()
        .zip(discovered)
        .flat_map(|(provider, accounts)| build_usage_tasks(provider, accounts))
        .collect();
    let accounts = map_with_concurrency(&tasks, tasks.len(), |task, _| read_task(sources, task));
    AiUsageSnapshot {
        accounts,
        fetched_at: sources.now_iso(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AiError;
    use crate::usage::types::{AiUsageStatus, AiUsageWindow, AiUsageWindowKind};
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// provider별 탐색 결과와 계정별 조회 결과를 정해 둔 원천
    #[derive(Default)]
    struct ScriptedSources {
        discovered: HashMap<AiUsageProvider, AiResult<Vec<AiUsageAccount>>>,
        failing_accounts: Vec<String>,
        read_accounts: Mutex<Vec<String>>,
    }

    impl UsageProviderSources for ScriptedSources {
        fn discover_accounts(
            &self,
            provider: AiUsageProvider,
            _: &[AiAccountRegistration],
        ) -> AiResult<Vec<AiUsageAccount>> {
            self.discovered
                .get(&provider)
                .cloned()
                .unwrap_or(Ok(Vec::new()))
        }

        fn read_usage(&self, account: &AiUsageAccount) -> AiResult<AiUsageAccountResult> {
            self.read_accounts
                .lock()
                .unwrap()
                .push(account.account_id.clone());
            if self.failing_accounts.contains(&account.account_id) {
                return Err(AiError::message("network down"));
            }
            Ok(create_ok_result(account))
        }

        fn now_iso(&self) -> String {
            "2026-08-10T00:00:00.000Z".to_string()
        }
    }

    fn create_account(provider: AiUsageProvider, account_id: &str) -> AiUsageAccount {
        AiUsageAccount {
            provider,
            account_id: account_id.to_string(),
            label: format!("{account_id}@example.com"),
            config_dir: format!("/home/tester/{account_id}"),
            account_root: format!("/home/tester/{account_id}"),
        }
    }

    fn create_ok_result(account: &AiUsageAccount) -> AiUsageAccountResult {
        AiUsageAccountResult {
            provider: account.provider,
            account_id: account.account_id.clone(),
            label: account.label.clone(),
            status: AiUsageStatus::Ok,
            plan_name: None,
            windows: vec![AiUsageWindow {
                kind: AiUsageWindowKind::Weekly,
                model_name: None,
                used_percent: 10.0,
                resets_at: None,
            }],
            reason: None,
            fetched_at: "2026-08-10T00:00:00.000Z".to_string(),
        }
    }

    fn result_for(snapshot: &AiUsageSnapshot, provider: AiUsageProvider) -> &AiUsageAccountResult {
        snapshot
            .accounts
            .iter()
            .find(|result| result.provider == provider)
            .unwrap()
    }

    #[test]
    fn providers_without_accounts_keep_a_card() {
        let sources = ScriptedSources::default();
        let snapshot = aggregate_ai_usage(&sources, &[]);
        let providers: Vec<_> = snapshot
            .accounts
            .iter()
            .map(|result| result.provider)
            .collect();
        assert_eq!(providers, AiUsageProvider::ALL);
        assert!(
            snapshot
                .accounts
                .iter()
                .all(|result| result.status == AiUsageStatus::Unavailable)
        );
        assert!(
            snapshot
                .accounts
                .iter()
                .all(|result| result.reason == Some(AiUsageFailureReason::MissingCredentials))
        );
        assert!(sources.read_accounts.lock().unwrap().is_empty());
    }

    #[test]
    fn each_account_gets_its_own_result() {
        let personal = create_account(AiUsageProvider::Claude, "personal");
        let work = create_account(AiUsageProvider::Claude, "work");
        let sources = ScriptedSources {
            discovered: HashMap::from([(AiUsageProvider::Claude, Ok(vec![personal, work]))]),
            ..Default::default()
        };
        let snapshot = aggregate_ai_usage(&sources, &[]);
        let claude_ids: Vec<&str> = snapshot
            .accounts
            .iter()
            .filter(|result| result.provider == AiUsageProvider::Claude)
            .map(|result| result.account_id.as_str())
            .collect();
        assert_eq!(claude_ids, ["personal", "work"]);
        let mut read_accounts = sources.read_accounts.lock().unwrap().clone();
        read_accounts.sort();
        assert_eq!(read_accounts, ["personal", "work"]);
    }

    #[test]
    fn one_failing_read_keeps_other_results() {
        let claude_account = create_account(AiUsageProvider::Claude, "personal");
        let sources = ScriptedSources {
            discovered: HashMap::from([
                (AiUsageProvider::Claude, Ok(vec![claude_account.clone()])),
                (
                    AiUsageProvider::Codex,
                    Ok(vec![create_account(AiUsageProvider::Codex, "codex-seat")]),
                ),
            ]),
            failing_accounts: vec!["personal".to_string()],
            ..Default::default()
        };
        let snapshot = aggregate_ai_usage(&sources, &[]);
        let claude_result = result_for(&snapshot, AiUsageProvider::Claude);
        assert_eq!(claude_result.status, AiUsageStatus::Error);
        assert_eq!(
            claude_result.reason,
            Some(AiUsageFailureReason::FetchFailed)
        );
        assert_eq!(claude_result.label, claude_account.label);
        assert_eq!(
            result_for(&snapshot, AiUsageProvider::Codex).status,
            AiUsageStatus::Ok
        );
    }

    #[test]
    fn failed_discovery_leaves_an_empty_slot() {
        let sources = ScriptedSources {
            discovered: HashMap::from([
                (
                    AiUsageProvider::Claude,
                    Err(AiError::message("home unreadable")),
                ),
                (
                    AiUsageProvider::Codex,
                    Ok(vec![create_account(AiUsageProvider::Codex, "codex-seat")]),
                ),
            ]),
            ..Default::default()
        };
        let snapshot = aggregate_ai_usage(&sources, &[]);
        assert_eq!(
            result_for(&snapshot, AiUsageProvider::Claude).status,
            AiUsageStatus::Unavailable
        );
        assert_eq!(
            result_for(&snapshot, AiUsageProvider::Codex).status,
            AiUsageStatus::Ok
        );
        assert!(
            regex::Regex::new(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T")
                .unwrap()
                .is_match(&snapshot.fetched_at)
        );
    }
}
