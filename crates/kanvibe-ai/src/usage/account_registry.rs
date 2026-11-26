//! 사용자가 KanVibe에서 만든 계정 목록. 탐색만으로는 로그아웃된 계정이 목록에서 사라져 다시 로그인할 자리가 없어지므로
//! "만들어 둔 계정"을 따로 기억하고 탐색 결과와 합쳐 자리를 지킨다.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::javascript::{parse_json, stringify};
use crate::usage::provider_config_dir::{
    config_dir_spec, is_valid_account_name, to_named_account_root,
};
use crate::usage::types::AiUsageProvider;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiAccountRegistration {
    pub provider: AiUsageProvider,
    /// CLI에 넘기는 계정 루트. 자격증명 디렉터리는 provider 규칙으로 유도한다
    pub account_root: String,
    /// 사용자가 붙인 계정 이름
    pub account_name: String,
}

fn to_registration(value: &Value) -> Option<AiAccountRegistration> {
    let provider = AiUsageProvider::parse(value.get("provider")?.as_str()?)?;
    let account_root = value
        .get("accountRoot")?
        .as_str()
        .filter(|root| !root.is_empty())?;
    let account_name = value
        .get("accountName")?
        .as_str()
        .filter(|name| is_valid_account_name(name))?;
    Some(AiAccountRegistration {
        provider,
        account_root: account_root.to_string(),
        account_name: account_name.to_string(),
    })
}

/// 저장된 계정 목록을 복원한다. 목록이 깨졌다고 사용량 화면 전체를 잃으면 안 되므로 해석되지 않는 항목은 버린다
pub fn parse_account_registrations(raw: Option<&str>) -> Vec<AiAccountRegistration> {
    let Some(raw) = raw.filter(|raw| !raw.is_empty()) else {
        return Vec::new();
    };
    match parse_json(raw) {
        Some(Value::Array(items)) => items.iter().filter_map(to_registration).collect(),
        _ => Vec::new(),
    }
}

pub fn serialize_account_registrations(registrations: &[AiAccountRegistration]) -> String {
    stringify(&serde_json::to_value(registrations).expect("계정 목록 직렬화"))
}

/// 같은 provider의 같은 이름은 같은 계정이므로 하나만 남긴다
pub fn add_account_registration(
    registrations: &[AiAccountRegistration],
    provider: AiUsageProvider,
    account_name: &str,
    home_directory: &str,
) -> Vec<AiAccountRegistration> {
    let account_root =
        to_named_account_root(config_dir_spec(provider), account_name, home_directory);
    let mut updated: Vec<AiAccountRegistration> = registrations
        .iter()
        .filter(|registration| registration.account_root != account_root)
        .cloned()
        .collect();
    updated.push(AiAccountRegistration {
        provider,
        account_root,
        account_name: account_name.to_string(),
    });
    updated
}

pub fn remove_account_registration(
    registrations: &[AiAccountRegistration],
    account_root: &str,
) -> Vec<AiAccountRegistration> {
    registrations
        .iter()
        .filter(|registration| registration.account_root != account_root)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const FAKE_HOME: &str = "/home/tester";

    fn create_registration() -> AiAccountRegistration {
        AiAccountRegistration {
            provider: AiUsageProvider::Claude,
            account_root: "/home/tester/.claude-work".to_string(),
            account_name: "work".to_string(),
        }
    }

    #[test]
    fn reads_back_what_was_saved() {
        let registrations = vec![create_registration()];
        let serialized = serialize_account_registrations(&registrations);
        assert_eq!(
            serialized,
            r#"[{"provider":"claude","accountRoot":"/home/tester/.claude-work","accountName":"work"}]"#
        );
        assert_eq!(
            parse_account_registrations(Some(&serialized)),
            registrations
        );
    }

    #[test]
    fn missing_value_is_empty() {
        assert!(parse_account_registrations(None).is_empty());
    }

    #[test]
    fn broken_json_is_dropped() {
        assert!(parse_account_registrations(Some("{not json")).is_empty());
    }

    #[test]
    fn keeps_only_readable_entries() {
        let raw = stringify(&json!([
            {"provider": "claude", "accountRoot": "/home/tester/.claude-work", "accountName": "work"},
            {"provider": "unknown", "accountRoot": "/x", "accountName": "x"},
            {"provider": "claude", "accountRoot": "/y", "accountName": "../escape"},
            {"provider": "codex", "accountName": "no-root"}
        ]));
        assert_eq!(
            parse_account_registrations(Some(&raw)),
            [create_registration()]
        );
    }

    #[test]
    fn add_builds_root_by_provider_rule() {
        assert_eq!(
            add_account_registration(&[], AiUsageProvider::Gemini, "work", FAKE_HOME),
            [AiAccountRegistration {
                provider: AiUsageProvider::Gemini,
                account_root: "/home/tester/.gemini-work".to_string(),
                account_name: "work".to_string(),
            }]
        );
    }

    #[test]
    fn adding_same_account_twice_keeps_one() {
        let first = add_account_registration(&[], AiUsageProvider::Claude, "work", FAKE_HOME);
        let second = add_account_registration(&first, AiUsageProvider::Claude, "work", FAKE_HOME);
        assert_eq!(second.len(), 1);
    }

    #[test]
    fn remove_drops_only_matching_root() {
        let registrations = add_account_registration(
            &add_account_registration(&[], AiUsageProvider::Claude, "work", FAKE_HOME),
            AiUsageProvider::Claude,
            "personal",
            FAKE_HOME,
        );
        let remaining = remove_account_registration(&registrations, "/home/tester/.claude-work");
        let names: Vec<&str> = remaining
            .iter()
            .map(|registration| registration.account_name.as_str())
            .collect();
        assert_eq!(names, ["personal"]);
    }
}
