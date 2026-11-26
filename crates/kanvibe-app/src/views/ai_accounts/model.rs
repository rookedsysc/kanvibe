//! AI 계정 화면 상태(TS `AiAccountsRoute` + `useAiUsage`의 상태 부분). 서비스 호출 없이 결과만 받아 바뀐다.

use kanvibe_ai::accounts::account_service::{
    AiAccountMutationOutcome, AiAccountMutationResult, AiAccountSummary,
};
use kanvibe_ai::usage::shared::SIGN_IN_FAILURE_REASONS;
use kanvibe_ai::usage::types::{AiUsageAccountResult, AiUsageProvider, AiUsageSnapshot};

/// 로그인 화면이 열려 있는 계정. 계정 루트가 세션을 가리킨다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveLogin {
    pub provider: AiUsageProvider,
    pub account_root: String,
}

/// 사용량 조회 상태(TS `useAiUsage`)
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UsageState {
    pub snapshot: Option<AiUsageSnapshot>,
    pub is_refreshing: bool,
    pub has_failed: bool,
}

impl UsageState {
    /// 보여줄 값이 하나도 없이 기다리는 중
    pub fn is_loading(&self) -> bool {
        self.snapshot.is_none() && self.is_refreshing
    }

    /// 저장된 값은 새 조회 결과가 아직 없을 때만 채운다
    pub fn show_cached(&mut self, cached: Option<AiUsageSnapshot>) {
        if self.snapshot.is_none() {
            self.snapshot = cached;
        }
    }

    /// 조회가 실패해도 보여주던 값은 남긴다. 빈 화면보다 오래된 값이 낫다
    pub fn finish_refresh(&mut self, result: Result<AiUsageSnapshot, String>) {
        self.is_refreshing = false;
        match result {
            Ok(snapshot) => {
                self.snapshot = Some(snapshot);
                self.has_failed = false;
            }
            Err(_) => self.has_failed = true,
        }
    }

    /// 계정 행에 붙일 사용량. 같은 이름의 계정도 ID(또는 캐시 ID)로 가른다
    pub fn usage_for(&self, account: &AiAccountSummary) -> Option<&AiUsageAccountResult> {
        self.snapshot.as_ref()?.accounts.iter().find(|result| {
            result.provider == account.provider
                && (result.account_id == account.account_id
                    || result.account_id == account.cached_account_id)
        })
    }
}

/// TS `AI_USAGE_REFRESH_INTERVAL_MS`: 사용량 자동 조회 주기
pub const USAGE_REFRESH_INTERVAL_MS: i64 = 5 * 60 * 1000;

/// 사용량 자동 조회 예약(TS `useAiUsage`의 `nextAutoLoad`·`refreshQueuedRef`·`useIsWindowActive`)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageRefreshSchedule {
    /// 다음 자동 조회 시각(epoch ms). 처음에는 0이라 창이 앞에 오면 바로 읽는다
    pub next_due_ms: i64,
    /// 조회 중에 새로고침이 들어왔다
    pub is_refresh_queued: bool,
    pub is_window_active: bool,
}

impl UsageRefreshSchedule {
    /// 결과가 만들어진 시각에서 주기를 센다. 같은 결과를 나눠 쓰는 창들이 같은 순간에 다시 조회하게 하려는 것이다.
    /// 실패했거나 시계가 틀어져 이미 지났으면 지금부터 센다
    pub fn set_next_due(&mut self, fetched_at_ms: Option<i64>, now_ms: i64) {
        self.next_due_ms = fetched_at_ms
            .map(|fetched_at| fetched_at + USAGE_REFRESH_INTERVAL_MS)
            .filter(|due| *due > now_ms)
            .unwrap_or(now_ms + USAGE_REFRESH_INTERVAL_MS);
    }

    pub fn wait_from(&self, now_ms: i64) -> std::time::Duration {
        std::time::Duration::from_millis((self.next_due_ms - now_ms).max(0) as u64)
    }
}

/// provider 카드 하나의 계정 추가 상태
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddAccountState {
    pub is_pending: bool,
    /// 서비스가 거절한 사유(`errors.<outcome>`)
    pub rejected_outcome: Option<AiAccountMutationOutcome>,
    /// 요청 자체가 실패한 사유. 같은 이름으로 다시 시도할 수 있다
    pub failure: Option<(String, String)>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AiAccountsModel {
    /// `None`이면 아직 한 번도 읽지 못했다
    pub accounts: Option<Vec<AiAccountSummary>>,
    pub load_failure: Option<String>,
    pub usage: UsageState,
    pub active_login: Option<ActiveLogin>,
    pub login_exit_code: Option<i32>,
    /// 지우는 중인 계정 루트(한 번에 하나)
    pub removing_root: Option<String>,
    /// 지우다 실패한 계정과 사유
    pub removal_failure: Option<(AiAccountSummary, String)>,
    pub add_states: [AddAccountState; 3],
}

pub fn provider_index(provider: AiUsageProvider) -> usize {
    AiUsageProvider::ALL
        .iter()
        .position(|candidate| *candidate == provider)
        .unwrap_or(0)
}

impl AiAccountsModel {
    /// 목록 조회 결과. 실패는 빈 목록과 구분해 사유로 남기고, 이미 보이던 목록은 지우지 않는다
    pub fn finish_list(&mut self, result: Result<Vec<AiAccountSummary>, String>) {
        match result {
            Ok(accounts) => {
                self.accounts = Some(accounts);
                self.load_failure = None;
            }
            Err(reason) => self.load_failure = Some(reason),
        }
    }

    pub fn accounts_of(&self, provider: AiUsageProvider) -> Vec<&AiAccountSummary> {
        self.accounts
            .iter()
            .flatten()
            .filter(|account| account.provider == provider)
            .collect()
    }

    /// 계정이 로그인이 필요한지. 계정이 로그인돼 있어도 사용량 조회가 자격증명 문제로 실패하면 로그인이 필요하다
    pub fn requires_login(&self, account: &AiAccountSummary) -> bool {
        let usage_reason = self.usage.usage_for(account).and_then(|usage| usage.reason);
        !account.is_logged_in
            || usage_reason.is_some_and(|reason| SIGN_IN_FAILURE_REASONS.contains(&reason))
    }

    pub fn open_login(&mut self, provider: AiUsageProvider, account_root: String) {
        self.login_exit_code = None;
        self.active_login = Some(ActiveLogin {
            provider,
            account_root,
        });
    }

    /// 로그인 창을 닫는다(정상 종료 또는 닫기 버튼)
    pub fn close_login(&mut self) {
        self.active_login = None;
        self.login_exit_code = None;
    }

    /// 로그인 명령이 끝났다. 정상 종료면 닫고 `true`, 아니면 출력을 남긴 채 종료 코드를 보인다
    pub fn finish_login(&mut self, exit_code: i32) -> bool {
        if exit_code == 0 {
            self.close_login();
            return true;
        }
        self.login_exit_code = Some(exit_code);
        false
    }

    /// 계정 추가를 시작한다. 진행 중이면 겹쳐 보내지 않는다
    pub fn begin_add(&mut self, provider: AiUsageProvider) -> bool {
        let state = &mut self.add_states[provider_index(provider)];
        if state.is_pending {
            return false;
        }
        *state = AddAccountState {
            is_pending: true,
            ..AddAccountState::default()
        };
        true
    }

    /// 계정 추가 결과. 만들었으면 그 계정 루트를 돌려준다(이어서 로그인 화면을 연다)
    pub fn finish_add(
        &mut self,
        provider: AiUsageProvider,
        name: &str,
        result: Result<AiAccountMutationResult, String>,
    ) -> Option<String> {
        let state = &mut self.add_states[provider_index(provider)];
        state.is_pending = false;
        match result {
            Ok(AiAccountMutationResult {
                outcome: AiAccountMutationOutcome::Ok,
                account_root: Some(account_root),
            }) => Some(account_root),
            Ok(rejected) => {
                state.rejected_outcome = Some(rejected.outcome);
                None
            }
            Err(reason) => {
                state.failure = Some((name.to_string(), reason));
                None
            }
        }
    }

    /// 한 번에 한 계정만 지운다
    pub fn begin_remove(&mut self, account: &AiAccountSummary) -> bool {
        if self.removing_root.is_some() {
            return false;
        }
        self.removing_root = Some(account.account_root.clone());
        self.removal_failure = None;
        true
    }

    pub fn finish_remove(&mut self, account: AiAccountSummary, result: Result<(), String>) {
        self.removing_root = None;
        if let Err(reason) = result {
            self.removal_failure = Some((account, reason));
        }
    }

    pub fn removal_failure_for(&self, account: &AiAccountSummary) -> Option<&str> {
        self.removal_failure
            .as_ref()
            .filter(|(failed, _)| failed.account_root == account.account_root)
            .map(|(_, reason)| reason.as_str())
    }
}

/// 사용자가 쓴 그대로 보이는 provider 이름(`capitalize`)
pub fn provider_title(provider: AiUsageProvider) -> &'static str {
    match provider {
        AiUsageProvider::Claude => "Claude",
        AiUsageProvider::Codex => "Codex",
        AiUsageProvider::Gemini => "Gemini",
    }
}

/// 실패 사유 i18n 키(`aiAccounts.errors.<키>`)
pub fn mutation_outcome_key(outcome: AiAccountMutationOutcome) -> &'static str {
    match outcome {
        AiAccountMutationOutcome::Ok => "ok",
        AiAccountMutationOutcome::InvalidName => "invalid-name",
        AiAccountMutationOutcome::AlreadyExists => "already-exists",
        AiAccountMutationOutcome::ManualLogoutRequired => "manual-logout-required",
    }
}
