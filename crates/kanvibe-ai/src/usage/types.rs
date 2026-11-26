//! 사용량 모델. 렌더러와 모바일로 나가는 값이라 TS와 같은 camelCase·kebab-case 이름을 쓴다.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiUsageProvider {
    Claude,
    Codex,
    Gemini,
}

impl AiUsageProvider {
    /// 화면 순서와 같은 provider 순서
    pub const ALL: [AiUsageProvider; 3] = [Self::Claude, Self::Codex, Self::Gemini];

    pub fn id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|provider| provider.id() == value)
    }
}

/// 사용량 창이 재는 기간. session은 5시간 창, weekly는 7일 창, monthly는 Codex 무료 등급의 30일 창이다.
/// model은 Gemini 모델별 쿼터처럼 묶일 기간 한도가 아예 없는 창에만 쓴다
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiUsageWindowKind {
    Session,
    Weekly,
    Monthly,
    Model,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiUsageStatus {
    Ok,
    Unavailable,
    Error,
}

/// 조회 실패 사유. 렌더러가 그대로 i18n 키로 쓰므로 사람이 읽는 문장이 아니라 안정적인 식별자를 담는다
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiUsageFailureReason {
    MissingCredentials,
    ExpiredCredentials,
    AwaitingCliRefresh,
    KeychainUnreadable,
    GeminiCliNotFound,
    RateLimited,
    EmptyResponse,
    FetchFailed,
}

/// 조회 대상 계정 하나. 경로는 조회와 로그인에만 쓰고 캐시에는 저장하지 않는다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiUsageAccount {
    pub provider: AiUsageProvider,
    pub account_id: String,
    pub label: String,
    /// 자격증명이 놓인 디렉터리
    pub config_dir: String,
    /// CLI에 계정 위치를 알릴 때 넘기는 루트. provider에 따라 config_dir와 같을 수도 있다
    pub account_root: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiUsageWindow {
    pub kind: AiUsageWindowKind,
    /// 모델별 창일 때만 채우는 표시용 모델 이름
    pub model_name: Option<String>,
    pub used_percent: f64,
    pub resets_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiUsageAccountResult {
    pub provider: AiUsageProvider,
    /// 발견 시점의 계정 식별자. 캐시에 넣을 때는 해시해서 전체 값을 남기지 않는다
    pub account_id: String,
    pub label: String,
    pub status: AiUsageStatus,
    /// 구독 등급 표시값. 응답이나 자격증명이 알려줄 때만 채운다
    pub plan_name: Option<String>,
    pub windows: Vec<AiUsageWindow>,
    pub reason: Option<AiUsageFailureReason>,
    pub fetched_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiUsageSnapshot {
    pub accounts: Vec<AiUsageAccountResult>,
    pub fetched_at: String,
}
