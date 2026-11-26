//! KanVibe에서 만든 AI 계정 목록과 계정 추가·삭제(TS `aiAccountService`). 로그인은 이어서 열리는 로그인 세션이 CLI에게 맡긴다.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::accounts::settings_store::AppSettingStore;
use crate::error::{AiError, AiResult};
use crate::session_listing::map_with_concurrency;
use crate::usage::account_discovery::{discover_provider_accounts, has_stored_credentials};
use crate::usage::account_registry::{
    AiAccountRegistration, add_account_registration, parse_account_registrations,
    remove_account_registration, serialize_account_registrations,
};
use crate::usage::context::UsageContext;
use crate::usage::provider_cli::{
    ProviderAuthStatus, logout_through_cli, read_provider_auth_status,
};
use crate::usage::provider_config_dir::{
    config_dir_spec, is_valid_account_name, to_account_config_dir, to_account_name_from_root,
    to_named_account_root,
};
use crate::usage::types::{AiUsageAccount, AiUsageProvider};
use crate::usage::usage_snapshot_cache::to_cached_account_id;

/// KanVibe에서 만든 AI 계정 목록. 로그아웃된 계정도 자리를 지키게 하는 유일한 근거다
pub const AI_ACCOUNTS_KEY: &str = "ai_accounts";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiAccountSummary {
    pub provider: AiUsageProvider,
    pub account_id: String,
    pub cached_account_id: String,
    /// CLI에 계정 위치를 알릴 때 넘기는 루트이자 이 목록에서 계정을 가리키는 식별자
    pub account_root: String,
    /// 사용자가 붙인 계정 이름. 홈의 기본 계정은 이름이 없다
    pub account_name: Option<String>,
    pub label: String,
    pub is_logged_in: bool,
    pub plan_name: Option<String>,
    /// KanVibe가 만든 계정만 앱에서 지울 수 있다
    pub is_removable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiAccountMutationOutcome {
    Ok,
    InvalidName,
    AlreadyExists,
    ManualLogoutRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiAccountMutationResult {
    pub outcome: AiAccountMutationOutcome,
    /// 계정을 만들었을 때만 채운다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_root: Option<String>,
}

impl AiAccountMutationResult {
    fn of(outcome: AiAccountMutationOutcome) -> Self {
        Self {
            outcome,
            account_root: None,
        }
    }
}

/// 계정 로그인 상태를 묻고 로그아웃을 맡기는 CLI. 앱에서는 [`UsageContext`]가 provider CLI를 실행한다
pub trait ProviderAccountCli: Send + Sync {
    fn read_auth_status(
        &self,
        provider: AiUsageProvider,
        account_root: &str,
    ) -> Option<ProviderAuthStatus>;
    fn logout(&self, provider: AiUsageProvider, account_root: &str) -> bool;
}

impl ProviderAccountCli for UsageContext {
    fn read_auth_status(
        &self,
        provider: AiUsageProvider,
        account_root: &str,
    ) -> Option<ProviderAuthStatus> {
        read_provider_auth_status(self, provider, account_root)
    }

    fn logout(&self, provider: AiUsageProvider, account_root: &str) -> bool {
        logout_through_cli(self, provider, account_root)
    }
}

pub fn get_ai_account_registrations(
    store: &dyn AppSettingStore,
) -> AiResult<Vec<AiAccountRegistration>> {
    Ok(parse_account_registrations(
        store.get_app_setting(AI_ACCOUNTS_KEY)?.as_deref(),
    ))
}

pub struct AiAccountService {
    store: Arc<dyn AppSettingStore>,
    context: UsageContext,
    cli: Arc<dyn ProviderAccountCli>,
    /// 계정 목록을 읽고 고쳐 쓰는 변경은 하나씩 차례로 처리한다. 앞 변경이 실패해도 다음 변경은 계속된다
    mutation_turn: Mutex<()>,
}

impl AiAccountService {
    pub fn new(
        store: Arc<dyn AppSettingStore>,
        context: UsageContext,
        cli: Arc<dyn ProviderAccountCli>,
    ) -> Self {
        Self {
            store,
            context,
            cli,
            mutation_turn: Mutex::new(()),
        }
    }

    pub fn get_ai_account_registrations(&self) -> AiResult<Vec<AiAccountRegistration>> {
        get_ai_account_registrations(self.store.as_ref())
    }

    fn save_ai_account_registrations(
        &self,
        registrations: &[AiAccountRegistration],
    ) -> AiResult<()> {
        self.store.set_app_setting(
            AI_ACCOUNTS_KEY,
            &serialize_account_registrations(registrations),
        )
    }

    /// CLI가 알려주는 상태를 우선하고, 물을 수 없거나 답하지 못한 경우만 자격증명 파일로 판단한다
    fn to_account_summary(
        &self,
        account: &AiUsageAccount,
        registrations: &[AiAccountRegistration],
    ) -> AiAccountSummary {
        let spec = config_dir_spec(account.provider);
        let auth_status = self
            .cli
            .read_auth_status(account.provider, &account.account_root);
        let is_logged_in = match &auth_status {
            Some(status) => status.is_logged_in,
            None => has_stored_credentials(spec, &account.config_dir),
        };
        let auth_label = auth_status.as_ref().and_then(|status| status.label.clone());
        AiAccountSummary {
            provider: account.provider,
            account_id: account.account_id.clone(),
            cached_account_id: to_cached_account_id(&account.account_id),
            account_root: account.account_root.clone(),
            account_name: to_account_name_from_root(spec, &account.account_root),
            label: auth_label.unwrap_or_else(|| account.label.clone()),
            is_logged_in,
            plan_name: auth_status.and_then(|status| status.plan_name),
            is_removable: registrations
                .iter()
                .any(|registration| registration.account_root == account.account_root),
        }
    }

    /// 화면 순서는 provider 순서가 정한다. 조회할 때마다 카드가 자리를 바꾸면 눈이 매번 다시 찾는다
    pub fn list_ai_accounts(&self) -> AiResult<Vec<AiAccountSummary>> {
        let registrations = self.get_ai_account_registrations()?;
        let accounts: Vec<AiUsageAccount> = AiUsageProvider::ALL
            .into_iter()
            .flat_map(|provider| {
                discover_provider_accounts(&self.context, provider, &registrations)
            })
            .collect();
        Ok(map_with_concurrency(
            &accounts,
            accounts.len(),
            |account, _| self.to_account_summary(account, &registrations),
        ))
    }

    fn lock_mutation_turn(&self) -> std::sync::MutexGuard<'_, ()> {
        self.mutation_turn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 계정 자리를 먼저 만들고 등록한다. 디렉터리를 미리 만들어 두는 것은 CLI가 그 자리를 자기 것으로 쓰게 하기 위해서다
    pub fn add_ai_account(
        &self,
        provider: AiUsageProvider,
        account_name: &str,
    ) -> AiResult<AiAccountMutationResult> {
        let _turn = self.lock_mutation_turn();
        if !is_valid_account_name(account_name) {
            return Ok(AiAccountMutationResult::of(
                AiAccountMutationOutcome::InvalidName,
            ));
        }
        let spec = config_dir_spec(provider);
        let home_directory = self.context.home_directory.as_str();
        let account_root = to_named_account_root(spec, account_name, home_directory);
        let registrations = self.get_ai_account_registrations()?;
        if registrations
            .iter()
            .any(|registration| registration.account_root == account_root)
        {
            return Ok(AiAccountMutationResult::of(
                AiAccountMutationOutcome::AlreadyExists,
            ));
        }
        let config_dir = to_account_config_dir(spec, &account_root);
        std::fs::create_dir_all(&config_dir)
            .map_err(|error| AiError::from_io(&error, &config_dir))?;
        self.save_ai_account_registrations(&add_account_registration(
            &registrations,
            provider,
            account_name,
            home_directory,
        ))?;
        Ok(AiAccountMutationResult {
            outcome: AiAccountMutationOutcome::Ok,
            account_root: Some(account_root),
        })
    }

    /// 로그아웃은 CLI에게 맡기고 KanVibe가 만든 계정 디렉터리만 지운다. 사용자가 직접 만든 디렉터리는 로그아웃까지만 하고 남긴다
    pub fn remove_ai_account(
        &self,
        provider: AiUsageProvider,
        account_root: &str,
    ) -> AiResult<AiAccountMutationResult> {
        let _turn = self.lock_mutation_turn();
        let registrations = self.get_ai_account_registrations()?;
        let is_registered = registrations
            .iter()
            .any(|registration| registration.account_root == account_root);
        let did_logout = self.cli.logout(provider, account_root);
        if is_registered {
            remove_directory_if_present(account_root)?;
            self.save_ai_account_registrations(&remove_account_registration(
                &registrations,
                account_root,
            ))?;
            return Ok(AiAccountMutationResult::of(AiAccountMutationOutcome::Ok));
        }
        Ok(AiAccountMutationResult::of(if did_logout {
            AiAccountMutationOutcome::Ok
        } else {
            AiAccountMutationOutcome::ManualLogoutRequired
        }))
    }
}

/// TS `rm(path, { recursive: true, force: true })`. 없는 경로는 성공으로 본다
fn remove_directory_if_present(target_path: &str) -> AiResult<()> {
    let result = match std::fs::symlink_metadata(target_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => Err(error),
        Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(target_path),
        Ok(_) => std::fs::remove_file(target_path),
    };
    result.map_err(|error| AiError::from_io(&error, target_path))
}

#[cfg(test)]
mod tests;
