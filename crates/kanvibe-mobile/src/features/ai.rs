//! 모바일 AI 기능(TS `mobileAiService`): 실행 중 세션·호출 그래프·계정 관리·사용량·계정 로그인 터미널.
//!
//! 계정은 디스크 경로 대신 `sha256(JSON([provider, accountRoot]))` 핸들로만 기기에 보인다. 기기가 보낸 핸들은
//! 지금 서버에 있는 계정으로 되찾아 쓰므로, 기기가 provider나 경로를 직접 지정할 길이 없다.

use std::sync::{Arc, Mutex, MutexGuard};

use kanvibe_ai::accounts::account_service::{AiAccountMutationOutcome, AiAccountSummary};
use kanvibe_ai::accounts::mobile_login_bridge::{
    MobileAiLoginError, MobileLoginOpenRequest, MobileLoginSocket,
};
use kanvibe_ai::javascript::{js_trim, utf16_length};
use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::types::{AgentCallGraph, LiveAiSessionsResult, RunningAgentPane};
use kanvibe_ai::usage::provider_config_dir::is_valid_account_name;
use kanvibe_ai::usage::types::{
    AiUsageFailureReason, AiUsageProvider, AiUsageSnapshot, AiUsageStatus, AiUsageWindowKind,
};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::{MobileError, MobileResult};
use crate::ports::{AiAccountDirectory, AiLoginSessions, AiUsageSource, ProjectCatalog, TaskBoard};

const MAX_IDENTIFIER_LENGTH: usize = 512;

pub struct MobileAiParts {
    pub tasks: Arc<dyn TaskBoard>,
    pub projects: Arc<dyn ProjectCatalog>,
    pub accounts: Arc<dyn AiAccountDirectory>,
    pub usage: Arc<dyn AiUsageSource>,
    pub logins: Arc<dyn AiLoginSessions>,
}

pub struct MobileAi {
    tasks: Arc<dyn TaskBoard>,
    projects: Arc<dyn ProjectCatalog>,
    accounts: Arc<dyn AiAccountDirectory>,
    usage: Arc<dyn AiUsageSource>,
    logins: Arc<dyn AiLoginSessions>,
    /// 삭제 중에 시작한 로그인 조회가 지워진 계정의 CLI를 다시 띄우지 않도록 두 진입점을 한 줄로 세운다
    account_operation_turn: Mutex<()>,
}

/// 기기에 보이는 계정. 경로 대신 불투명한 핸들만 싣는다
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileAiAccount {
    pub account_id: String,
    pub provider: AiUsageProvider,
    pub account_name: Option<String>,
    pub label: String,
    pub is_logged_in: bool,
    pub plan_name: Option<String>,
    pub is_removable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileAiAccountCreated {
    pub account_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MobileAiAccountRemoval {
    pub outcome: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileAiUsageWindow {
    pub kind: AiUsageWindowKind,
    pub model_name: Option<String>,
    pub used_percent: f64,
    pub resets_at: Option<String>,
}

/// 사용량의 원본 계정 ID는 provider UUID나 디스크 경로라 관리용 핸들과 같은 방식으로 요약한다
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileAiUsageAccount {
    pub provider: AiUsageProvider,
    pub account_id: String,
    pub label: String,
    pub status: AiUsageStatus,
    pub plan_name: Option<String>,
    pub reason: Option<AiUsageFailureReason>,
    pub fetched_at: String,
    pub windows: Vec<MobileAiUsageWindow>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileAiUsageSnapshot {
    pub fetched_at: String,
    pub accounts: Vec<MobileAiUsageAccount>,
}

fn account_handle(provider: &str, account_root: &str) -> String {
    let identity = serde_json::to_string(&[provider, account_root]).unwrap_or_default();
    hex::encode(Sha256::digest(identity.as_bytes()))
}

fn summary_handle(account: &AiAccountSummary) -> String {
    account_handle(account.provider.id(), &account.account_root)
}

fn is_account_handle(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// 비어 있지 않고 512자(UTF-16 단위) 이하이며 제어 문자가 없는 식별자
fn validate_identifier(value: &str) -> MobileResult<()> {
    let is_valid = !js_trim(value).is_empty()
        && utf16_length(value) <= MAX_IDENTIFIER_LENGTH
        && !value.chars().any(|character| character <= '\u{1f}');
    if !is_valid {
        return Err(MobileError::ai_request(400, "식별자가 올바르지 않습니다"));
    }
    Ok(())
}

fn to_ai_error(error: MobileAiLoginError) -> MobileError {
    MobileError::ai_request(error.status_code, error.message)
}

/// 계정 추가 본문은 `provider`·`accountName` 두 칸만 받는다
fn read_new_account(input: &Value) -> MobileResult<(AiUsageProvider, String)> {
    let values = input
        .as_object()
        .ok_or_else(|| MobileError::ai_request(400, "계정 정보를 확인해 주세요"))?;
    let has_only_known_keys = values
        .keys()
        .all(|key| key == "provider" || key == "accountName");
    let provider = values
        .get("provider")
        .and_then(Value::as_str)
        .and_then(|provider| {
            AiUsageProvider::ALL
                .into_iter()
                .find(|candidate| candidate.id() == provider)
        });
    let account_name = values
        .get("accountName")
        .and_then(Value::as_str)
        .filter(|name| is_valid_account_name(name));
    match (has_only_known_keys, provider, account_name) {
        (true, Some(provider), Some(account_name)) => Ok((provider, account_name.to_string())),
        _ => Err(MobileError::ai_request(
            400,
            "제공자와 계정 이름을 확인해 주세요",
        )),
    }
}

impl MobileAi {
    pub fn new(parts: MobileAiParts) -> Self {
        Self {
            tasks: parts.tasks,
            projects: parts.projects,
            accounts: parts.accounts,
            usage: parts.usage,
            logins: parts.logins,
            account_operation_turn: Mutex::new(()),
        }
    }

    fn account_turn(&self) -> MutexGuard<'_, ()> {
        self.account_operation_turn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn list_accounts(&self) -> MobileResult<Vec<AiAccountSummary>> {
        self.accounts
            .list_ai_accounts()
            .map_err(MobileError::Internal)
    }

    fn find_mobile_account(&self, account_id: &str) -> MobileResult<AiAccountSummary> {
        if !is_account_handle(account_id) {
            return Err(MobileError::ai_request(
                400,
                "계정 식별자가 올바르지 않습니다",
            ));
        }
        self.list_accounts()?
            .into_iter()
            .find(|account| summary_handle(account) == account_id)
            .ok_or_else(|| MobileError::ai_request(404, "계정을 찾을 수 없습니다"))
    }

    fn require_task(&self, task_id: &str) -> MobileResult<()> {
        validate_identifier(task_id)?;
        if self.tasks.get_task_by_id(task_id)?.is_none() {
            return Err(MobileError::ai_request(404, "태스크를 찾을 수 없습니다"));
        }
        Ok(())
    }

    pub fn get_mobile_live_ai_sessions(&self, task_id: &str) -> MobileResult<LiveAiSessionsResult> {
        self.require_task(task_id)?;
        Ok(self.projects.get_task_live_ai_sessions(task_id)?)
    }

    /// 세션 ID는 기록 파일 이름이 되므로 경로 구분자와 `.`·`..`를 거절한다
    pub fn get_mobile_agent_call_graph(
        &self,
        task_id: &str,
        provider: &str,
        session_id: &str,
    ) -> MobileResult<Option<AgentCallGraph>> {
        let Some(provider) = AiSessionProvider::parse(provider) else {
            return Err(MobileError::ai_request(
                400,
                "지원하지 않는 AI 제공자입니다",
            ));
        };
        validate_identifier(session_id)?;
        if session_id.contains(['/', '\\']) || session_id == "." || session_id == ".." {
            return Err(MobileError::ai_request(
                400,
                "AI 세션 식별자가 올바르지 않습니다",
            ));
        }
        self.require_task(task_id)?;
        Ok(self
            .projects
            .get_task_agent_call_graph(task_id, provider, session_id)?)
    }

    pub fn list_mobile_ai_accounts(&self) -> MobileResult<Vec<MobileAiAccount>> {
        Ok(self
            .list_accounts()?
            .into_iter()
            .map(|account| MobileAiAccount {
                account_id: summary_handle(&account),
                provider: account.provider,
                account_name: account.account_name,
                label: account.label,
                is_logged_in: account.is_logged_in,
                plan_name: account.plan_name,
                is_removable: account.is_removable,
            })
            .collect())
    }

    pub fn add_mobile_ai_account(&self, input: &Value) -> MobileResult<MobileAiAccountCreated> {
        let (provider, account_name) = read_new_account(input)?;
        let result = self
            .accounts
            .add_ai_account(provider, &account_name)
            .map_err(MobileError::Internal)?;
        if result.outcome == AiAccountMutationOutcome::AlreadyExists {
            return Err(MobileError::ai_request(409, "이미 등록된 계정 이름입니다"));
        }
        match (result.outcome, result.account_root) {
            (AiAccountMutationOutcome::Ok, Some(account_root)) => Ok(MobileAiAccountCreated {
                account_id: account_handle(provider.id(), &account_root),
            }),
            _ => Err(MobileError::ai_request(400, "계정을 추가하지 못했습니다")),
        }
    }

    /// KanVibe가 만든 계정만 지운다. 지우기 전에 그 계정을 붙든 로그인 터미널을 닫는다
    pub fn remove_mobile_ai_account(
        &self,
        account_id: &str,
    ) -> MobileResult<MobileAiAccountRemoval> {
        let _turn = self.account_turn();
        let account = self.find_mobile_account(account_id)?;
        if !account.is_removable {
            return Err(MobileError::ai_request(
                403,
                "KanVibe에서 추가한 계정만 삭제할 수 있습니다",
            ));
        }
        self.logins.close_for_account(&account.account_root);
        let result = self
            .accounts
            .remove_ai_account(account.provider, &account.account_root)
            .map_err(MobileError::Internal)?;
        if result.outcome != AiAccountMutationOutcome::Ok {
            return Err(MobileError::ai_request(
                409,
                "PC에서 계정 로그아웃을 먼저 완료해 주세요",
            ));
        }
        Ok(MobileAiAccountRemoval { outcome: "ok" })
    }

    pub fn get_mobile_ai_usage_snapshot(&self) -> MobileResult<MobileAiUsageSnapshot> {
        let snapshot: AiUsageSnapshot = self
            .usage
            .get_ai_usage_snapshot()
            .map_err(MobileError::Internal)?;
        let accounts = snapshot
            .accounts
            .into_iter()
            .map(|account| MobileAiUsageAccount {
                account_id: account_handle(account.provider.id(), &account.account_id),
                provider: account.provider,
                label: account.label,
                status: account.status,
                plan_name: account.plan_name,
                reason: account.reason,
                fetched_at: account.fetched_at,
                windows: account
                    .windows
                    .into_iter()
                    .map(|window| MobileAiUsageWindow {
                        kind: window.kind,
                        model_name: window.model_name,
                        used_percent: window.used_percent,
                        resets_at: window.resets_at,
                    })
                    .collect(),
            })
            .collect();
        Ok(MobileAiUsageSnapshot {
            fetched_at: snapshot.fetched_at,
            accounts,
        })
    }

    /// 핸들로 되찾은 서버 계정으로만 로그인 터미널을 연다. 조회하는 사이 소켓이 닫혔으면 띄우지 않는다
    pub fn open_mobile_ai_login(
        &self,
        device_id: &str,
        socket_id: &str,
        account_id: &str,
        size: (u16, u16),
        socket: Arc<dyn MobileLoginSocket>,
    ) -> MobileResult<()> {
        let _turn = self.account_turn();
        let account = self.find_mobile_account(account_id)?;
        if !socket.is_open() {
            return Ok(());
        }
        let (columns, rows) = size;
        self.logins
            .open(MobileLoginOpenRequest {
                device_id: device_id.to_string(),
                socket_id: socket_id.to_string(),
                provider: account.provider,
                account_root: account.account_root,
                columns: f64::from(columns),
                rows: f64::from(rows),
                socket,
            })
            .map_err(to_ai_error)
    }

    pub fn write_mobile_ai_login(
        &self,
        device_id: &str,
        socket_id: &str,
        data: &str,
    ) -> MobileResult<()> {
        self.logins
            .write(device_id, socket_id, data)
            .map_err(to_ai_error)
    }

    pub fn resize_mobile_ai_login(
        &self,
        device_id: &str,
        socket_id: &str,
        size: (u16, u16),
    ) -> MobileResult<()> {
        self.logins
            .resize(device_id, socket_id, f64::from(size.0), f64::from(size.1))
            .map_err(to_ai_error)
    }

    pub fn close_mobile_ai_login(&self, device_id: &str, socket_id: &str) {
        self.logins.close(device_id, socket_id);
    }

    pub fn close_mobile_ai_logins_for_device(&self, device_id: &str) {
        self.logins.close_for_device(device_id);
    }

    pub fn close_all_mobile_ai_logins(&self) {
        self.logins.close_all();
    }

    pub fn get_mobile_running_agent_panes(&self) -> Vec<RunningAgentPane> {
        self.projects.get_running_agent_panes()
    }
}

#[cfg(test)]
mod tests;
