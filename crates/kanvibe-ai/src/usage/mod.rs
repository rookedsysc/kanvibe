//! AI 사용량 계층. TS `src/lib/aiUsage/**`를 파일 하나당 모듈 하나로 옮긴다.
//!
//! 네트워크, 하위 프로세스, 시계, 홈 디렉터리와 환경 변수는 모두 [`context::UsageContext`]로 주입한다.

pub mod account_discovery;
pub mod account_registry;
pub mod claude_credentials;
pub mod claude_usage;
pub mod codex_usage;
pub mod context;
pub mod gemini_oauth_client;
pub mod gemini_usage;
pub mod provider_cli;
pub mod provider_config_dir;
pub mod shared;
pub mod system_transport;
pub mod types;
pub mod usage_aggregation;
pub mod usage_snapshot_cache;

#[cfg(test)]
pub(crate) mod test_context;
