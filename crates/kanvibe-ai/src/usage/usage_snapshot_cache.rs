//! 사용량 스냅샷을 앱 설정에 남기는 형식. 앱을 껐다 켜도 패널이 빈 화면으로 열리지 않게 하는 편의값이라,
//! 조금이라도 해석이 어긋나면 고쳐 쓰지 않고 버린다.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::javascript::{parse_json, stringify};
use crate::usage::types::{AiUsageAccountResult, AiUsageSnapshot, AiUsageWindow};

/// 저장 형식이 바뀌면 올린다. 값이 다른 캐시는 해석하지 않고 버린다
const CACHE_SCHEMA_VERSION: u64 = 1;
const CACHED_ACCOUNT_ID_LENGTH: usize = 16;

const KNOWN_PROVIDERS: [&str; 3] = ["claude", "codex", "gemini"];
const KNOWN_STATUSES: [&str; 3] = ["ok", "unavailable", "error"];
const KNOWN_WINDOW_KINDS: [&str; 4] = ["session", "weekly", "monthly", "model"];

/// 계정 UUID와 config dir 경로는 화면이 쓰지 않는데 디스크에는 남으므로 되돌릴 수 없는 요약값으로 바꿔 저장한다
pub fn to_cached_account_id(account_id: &str) -> String {
    let hex: String = Sha256::digest(account_id.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    hex[..CACHED_ACCOUNT_ID_LENGTH].to_string()
}

/// 필드를 하나씩 골라 담는다. 결과 타입에 토큰 같은 값이 붙어도 캐시로 새어 나가지 않는다
fn to_cacheable_account(account: &AiUsageAccountResult) -> AiUsageAccountResult {
    AiUsageAccountResult {
        account_id: to_cached_account_id(&account.account_id),
        ..account.clone()
    }
}

pub fn to_cacheable_snapshot(snapshot: &AiUsageSnapshot) -> String {
    let cacheable = AiUsageSnapshot {
        accounts: snapshot.accounts.iter().map(to_cacheable_account).collect(),
        fetched_at: snapshot.fetched_at.clone(),
    };
    let envelope = json!({
        "version": CACHE_SCHEMA_VERSION,
        "snapshot": serde_json::to_value(cacheable).expect("스냅샷 직렬화"),
    });
    stringify(&envelope)
}

fn is_string_or_null(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::String(_) | Value::Null))
}

fn is_known(value: Option<&Value>, known: &[&str]) -> bool {
    value
        .and_then(Value::as_str)
        .is_some_and(|text| known.contains(&text))
}

fn is_cached_window(value: &Value) -> bool {
    is_known(value.get("kind"), &KNOWN_WINDOW_KINDS)
        && value.get("usedPercent").is_some_and(Value::is_number)
        && is_string_or_null(value.get("modelName"))
        && is_string_or_null(value.get("resetsAt"))
}

fn is_cached_account(value: &Value) -> bool {
    let is_string = |name: &str| value.get(name).is_some_and(Value::is_string);
    is_known(value.get("provider"), &KNOWN_PROVIDERS)
        && is_known(value.get("status"), &KNOWN_STATUSES)
        && is_string("accountId")
        && is_string("label")
        && is_string("fetchedAt")
        && value
            .get("windows")
            .and_then(Value::as_array)
            .is_some_and(|windows| windows.iter().all(is_cached_window))
}

/// 저장된 스냅샷을 복원한다. 값이 없거나 형식이 맞지 않으면 `None`이다
pub fn from_cached_snapshot(raw: Option<&str>) -> Option<AiUsageSnapshot> {
    let envelope = parse_json(raw.filter(|raw| !raw.is_empty())?)?;
    if envelope.get("version").and_then(Value::as_f64) != Some(CACHE_SCHEMA_VERSION as f64) {
        return None;
    }
    let snapshot = envelope
        .get("snapshot")
        .filter(|snapshot| snapshot.is_object())?;
    let accounts = snapshot.get("accounts")?.as_array()?;
    let is_valid = snapshot.get("fetchedAt").is_some_and(Value::is_string)
        && accounts.iter().all(is_cached_account);
    if !is_valid {
        return None;
    }
    Some(AiUsageSnapshot {
        accounts: accounts
            .iter()
            .map(to_restored_account)
            .collect::<Option<Vec<_>>>()?,
        fetched_at: snapshot["fetchedAt"].as_str()?.to_string(),
    })
}

/// 검사를 통과한 항목을 모델로 옮긴다. 구독 등급과 사유는 문자열·알려진 값일 때만 살린다
fn to_restored_account(value: &Value) -> Option<AiUsageAccountResult> {
    let windows: Vec<AiUsageWindow> = serde_json::from_value(value["windows"].clone()).ok()?;
    Some(AiUsageAccountResult {
        provider: serde_json::from_value(value["provider"].clone()).ok()?,
        account_id: value["accountId"].as_str()?.to_string(),
        label: value["label"].as_str()?.to_string(),
        status: serde_json::from_value(value["status"].clone()).ok()?,
        plan_name: value
            .get("planName")
            .and_then(Value::as_str)
            .map(str::to_string),
        windows,
        reason: value
            .get("reason")
            .and_then(|reason| serde_json::from_value(reason.clone()).ok()),
        fetched_at: value["fetchedAt"].as_str()?.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::types::{AiUsageProvider, AiUsageStatus, AiUsageWindowKind};

    fn create_snapshot() -> AiUsageSnapshot {
        AiUsageSnapshot {
            fetched_at: "2026-08-10T06:00:00.000Z".to_string(),
            accounts: vec![AiUsageAccountResult {
                provider: AiUsageProvider::Claude,
                account_id: "e1c9e6ac-0000-4000-8000-000000000000".to_string(),
                label: "me@example.com".to_string(),
                status: AiUsageStatus::Ok,
                plan_name: Some("max".to_string()),
                windows: vec![window(AiUsageWindowKind::Session, None, 22.0)],
                reason: None,
                fetched_at: "2026-08-10T06:00:00.000Z".to_string(),
            }],
        }
    }

    fn window(
        kind: AiUsageWindowKind,
        model_name: Option<&str>,
        used_percent: f64,
    ) -> AiUsageWindow {
        AiUsageWindow {
            kind,
            model_name: model_name.map(str::to_string),
            used_percent,
            resets_at: None,
        }
    }

    fn mutate_stored(edit: impl FnOnce(&mut Value)) -> String {
        let mut stored = parse_json(&to_cacheable_snapshot(&create_snapshot())).unwrap();
        edit(&mut stored);
        stringify(&stored)
    }

    #[test]
    fn account_id_is_not_stored_verbatim() {
        assert!(
            !to_cacheable_snapshot(&create_snapshot())
                .contains("e1c9e6ac-0000-4000-8000-000000000000")
        );
    }

    #[test]
    fn same_account_keeps_same_cached_id() {
        let first = from_cached_snapshot(Some(&to_cacheable_snapshot(&create_snapshot()))).unwrap();
        let second =
            from_cached_snapshot(Some(&to_cacheable_snapshot(&create_snapshot()))).unwrap();
        assert_eq!(first.accounts[0].account_id, second.accounts[0].account_id);
    }

    #[test]
    fn monthly_and_model_windows_round_trip() {
        let mut snapshot = create_snapshot();
        snapshot.accounts[0].windows = vec![
            window(AiUsageWindowKind::Monthly, None, 29.0),
            window(AiUsageWindowKind::Model, Some("Fable"), 4.0),
        ];
        let restored = from_cached_snapshot(Some(&to_cacheable_snapshot(&snapshot))).unwrap();
        let kinds: Vec<_> = restored.accounts[0]
            .windows
            .iter()
            .map(|window| window.kind)
            .collect();
        assert_eq!(
            kinds,
            [AiUsageWindowKind::Monthly, AiUsageWindowKind::Model]
        );
    }

    #[test]
    fn rendered_values_survive() {
        let restored =
            from_cached_snapshot(Some(&to_cacheable_snapshot(&create_snapshot()))).unwrap();
        assert_eq!(restored.fetched_at, "2026-08-10T06:00:00.000Z");
        let account = &restored.accounts[0];
        assert_eq!(account.provider, AiUsageProvider::Claude);
        assert_eq!(account.label, "me@example.com");
        assert_eq!(account.status, AiUsageStatus::Ok);
        assert_eq!(account.plan_name.as_deref(), Some("max"));
        assert_eq!(account.reason, None);
        assert_eq!(
            account.windows,
            [window(AiUsageWindowKind::Session, None, 22.0)]
        );
    }

    #[test]
    fn only_known_fields_are_stored() {
        let serialized = to_cacheable_snapshot(&create_snapshot());
        assert_eq!(
            serialized,
            format!(
                r#"{{"version":1,"snapshot":{{"accounts":[{{"provider":"claude","accountId":"{}","label":"me@example.com","status":"ok","planName":"max","windows":[{{"kind":"session","modelName":null,"usedPercent":22,"resetsAt":null}}],"reason":null,"fetchedAt":"2026-08-10T06:00:00.000Z"}}],"fetchedAt":"2026-08-10T06:00:00.000Z"}}}}"#,
                to_cached_account_id("e1c9e6ac-0000-4000-8000-000000000000")
            )
        );
    }

    #[test]
    fn missing_value_is_none() {
        assert_eq!(from_cached_snapshot(None), None);
        assert_eq!(from_cached_snapshot(Some("")), None);
    }

    #[test]
    fn broken_json_is_dropped() {
        assert_eq!(from_cached_snapshot(Some("{not json")), None);
    }

    #[test]
    fn other_schema_version_is_dropped() {
        let stored = mutate_stored(|stored| stored["version"] = json!(99));
        assert_eq!(from_cached_snapshot(Some(&stored)), None);
    }

    #[test]
    fn unknown_provider_or_status_is_dropped() {
        let stored =
            mutate_stored(|stored| stored["snapshot"]["accounts"][0]["provider"] = json!("grok"));
        assert_eq!(from_cached_snapshot(Some(&stored)), None);
    }
}
