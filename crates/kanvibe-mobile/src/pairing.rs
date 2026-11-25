//! 모바일 기기를 데스크탑에 한 번 연결하고, 그 뒤로는 저장된 토큰으로 계속 인증하는 규칙(TS `lib/mobilePairing.ts`).
//!
//! 페어링 코드는 짧고 사람이 옮겨 적는 값이라 화면에 떠 있는 동안에만 살아 있고 한 번 쓰면 사라진다.
//! 반대로 기기 토큰은 길고 기기에만 저장되므로 만료시키지 않는다. 한 번 연결한 기기는 계속 쓸 수 있어야 한다.
//! 아직 쓰이지 않은 코드는 TS 모듈 지역 상태 대신 [`PairingCodes`]가 들고, 기기 목록의 영속화는 `devices`가 맡는다.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use kanvibe_ai::javascript::{is_js_whitespace, utf16_length};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 사람이 화면을 보고 옮겨 적는 값이라 자릿수를 늘리지 않는다. 대신 수명이 짧고 한 번만 쓰인다
const PAIRING_CODE_DIGITS: usize = 6;
const PAIRING_CODE_SPACE: u32 = 1_000_000;

/// 코드가 화면에 떠 있을 법한 시간. hook 설치 티켓과 같은 값을 쓴다
pub const PAIRING_CODE_TTL_MS: i64 = 5 * 60_000;

/// 코드 하나가 견디는 실패 횟수. 넘어서면 코드를 버려 데스크탑에서 새로 받아야 한다.
/// 이 상한이 없으면 코드 수명 5분 안에 LAN의 누구나 전체 공간을 훑어 만료되지 않는 기기 토큰을 가져간다
pub const PAIRING_CODE_MAX_FAILURES: u32 = 5;

/// 실패한 시도만 이만큼 늦춘다. 성공은 늦추지 않는다
pub const PAIRING_CODE_FAILURE_DELAY: Duration = Duration::from_millis(300);

/// 기기 토큰은 저장되어 계속 쓰이므로 추측 시도를 무의미하게 만들 만큼 길어야 한다
const DEVICE_TOKEN_BYTES: usize = 32;
const DEVICE_ID_BYTES: usize = 8;
const MAX_PUSH_TOKEN_LENGTH: usize = 4096;
const MAX_CONNECTION_ID_LENGTH: usize = 128;

/// 실패한 교환을 늦추는 방법. 앱은 스레드를 재우고 테스트는 요청된 시간만 기록한다
pub type FailureDelay = dyn Fn(Duration) + Send + Sync;

pub fn thread_sleep_delay() -> Arc<FailureDelay> {
    Arc::new(std::thread::sleep)
}

/// 화면에 띄울 페어링 코드
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssuedPairingCode {
    pub code: String,
    pub expires_at: i64,
}

struct PendingPairing {
    code: String,
    expires_at: i64,
    /// 이 코드로 틀린 횟수. 코드마다 따로 세므로 새로 발급하면 0에서 다시 시작한다
    failed_attempts: u32,
}

/// 아직 교환되지 않은 페어링 코드. 앱이 꺼지면 같이 사라지는 것이 맞다
pub struct PairingCodes {
    pending: Mutex<Option<PendingPairing>>,
    failure_delay: Arc<FailureDelay>,
}

fn random_bytes<const LENGTH: usize>() -> [u8; LENGTH] {
    let mut bytes = [0u8; LENGTH];
    getrandom::fill(&mut bytes).expect("운영체제 난수를 얻지 못했습니다");
    bytes
}

/// 앞자리 0이 있는 6자리 코드. 난수를 십진수로 접으면 값이 고르지 않아지므로 범위를 넘는 표본은 버리고 다시 뽑는다
fn generate_pairing_code() -> String {
    let ceiling = (u32::MAX / PAIRING_CODE_SPACE) * PAIRING_CODE_SPACE;
    loop {
        let sample = u32::from_be_bytes(random_bytes::<4>());
        if sample < ceiling {
            return format!(
                "{:0width$}",
                sample % PAIRING_CODE_SPACE,
                width = PAIRING_CODE_DIGITS
            );
        }
    }
}

impl PairingCodes {
    pub fn new(failure_delay: Arc<FailureDelay>) -> Self {
        Self {
            pending: Mutex::new(None),
            failure_delay,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Option<PendingPairing>> {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 코드는 한 번에 하나만 살아 있다. 여러 개를 동시에 열어 두면 어느 것이 화면의 값인지 사용자가 알 수 없다
    pub fn issue(&self, now_ms: i64) -> IssuedPairingCode {
        let pending = PendingPairing {
            code: generate_pairing_code(),
            expires_at: now_ms + PAIRING_CODE_TTL_MS,
            failed_attempts: 0,
        };
        let issued = IssuedPairingCode {
            code: pending.code.clone(),
            expires_at: pending.expires_at,
        };
        *self.lock() = Some(pending);
        issued
    }

    /// 페어링 화면을 닫으면 코드도 죽어야 한다
    pub fn revoke(&self) {
        *self.lock() = None;
    }

    /// 화면에 떠 있는 코드. 없거나 만료됐으면 `None`
    pub fn read_pending(&self, now_ms: i64) -> Option<String> {
        read_live_code(&self.lock(), now_ms)
    }

    /// 코드를 확인하고 성공하면 소모한다. 실패는 지연보다 먼저 세어, 동시에 몰려온 시도가 잠든 사이에도 상한이 지켜진다.
    /// 상한에 닿아 버린 코드는 맞는 값을 넣어도 통하지 않는다
    pub fn redeem(&self, submitted_code: &str, now_ms: i64) -> bool {
        let mut pending = self.lock();
        let is_match = read_live_code(&pending, now_ms)
            .is_some_and(|expected| is_equal_secret(submitted_code, &expected));
        if is_match {
            *pending = None;
            return true;
        }
        count_failed_redeem(&mut pending);
        drop(pending);
        (self.failure_delay)(PAIRING_CODE_FAILURE_DELAY);
        false
    }
}

fn read_live_code(pending: &Option<PendingPairing>, now_ms: i64) -> Option<String> {
    pending
        .as_ref()
        .filter(|pairing| pairing.expires_at > now_ms)
        .map(|pairing| pairing.code.clone())
}

fn count_failed_redeem(pending: &mut Option<PendingPairing>) {
    let Some(pairing) = pending.as_mut() else {
        return;
    };
    pairing.failed_attempts += 1;
    if pairing.failed_attempts >= PAIRING_CODE_MAX_FAILURES {
        *pending = None;
    }
}

/// 기기가 저장하고 이후 모든 요청에 실어 보낼 토큰
pub fn mint_device_token() -> String {
    hex::encode(random_bytes::<DEVICE_TOKEN_BYTES>())
}

/// 기기 식별자. 추측을 막을 필요가 없으므로 토큰보다 짧다
pub fn mint_device_id() -> String {
    hex::encode(random_bytes::<DEVICE_ID_BYTES>())
}

/// 길이가 다르면 비교 자체가 성립하지 않으므로 먼저 거른다.
/// 길이가 같을 때는 모든 바이트를 끝까지 비교해 앞자리부터 몇 글자가 맞았는지가 응답 시간으로 새지 않게 한다
fn is_equal_secret(candidate: &str, expected: &str) -> bool {
    let (candidate, expected) = (candidate.as_bytes(), expected.as_bytes());
    if candidate.len() != expected.len() {
        return false;
    }
    let difference = candidate
        .iter()
        .zip(expected)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right));
    difference == 0
}

/// 연결된 기기 하나. `token`은 기기가 보관하고 데스크탑은 대조용으로만 가진다
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDevice {
    /// 기기를 가리키는 식별자. 비밀값이 아니므로 화면과 목록에 그대로 실어 보낸다
    pub device_id: String,
    pub token: String,
    pub device_name: String,
    pub paired_at: String,
    /// 저장된 푸시 구독 원본. 깨진 값도 페어링과 함께 보존하고 발송 대상에서만 뺀다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PushPlatform {
    Android,
    Ios,
}

/// 페어링과 같은 수명으로 저장하는 푸시 구독. 연결을 끊으면 함께 사라진다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MobilePushSubscription {
    pub push_token: String,
    pub platform: PushPlatform,
    pub connection_id: String,
}

/// JS `/^\S{1,max}$/`. 길이는 UTF-16 단위로 센다
fn is_compact_text(value: &str, max_length: usize) -> bool {
    let length = utf16_length(value);
    (1..=max_length).contains(&length) && !value.chars().any(is_js_whitespace)
}

/// 저장된 구독 값을 읽는다. 모양이 맞지 않으면 `None`이라 그 구독만 발송 대상에서 빠진다
pub fn read_mobile_push_subscription(value: &Value) -> Option<MobilePushSubscription> {
    let push_token = value.get("pushToken")?.as_str()?;
    let connection_id = value.get("connectionId")?.as_str()?;
    let platform = match value.get("platform")?.as_str()? {
        "android" => PushPlatform::Android,
        "ios" => PushPlatform::Ios,
        _ => return None,
    };
    let is_valid = is_compact_text(push_token, MAX_PUSH_TOKEN_LENGTH)
        && is_compact_text(connection_id, MAX_CONNECTION_ID_LENGTH)
        && connection_id.len() <= MAX_CONNECTION_ID_LENGTH;
    is_valid.then(|| MobilePushSubscription {
        push_token: push_token.to_string(),
        platform,
        connection_id: connection_id.to_string(),
    })
}

/// 저장된 기기 목록에서 토큰의 주인을 찾는다
pub fn find_paired_device<'a>(
    devices: &'a [PairedDevice],
    token: Option<&str>,
) -> Option<&'a PairedDevice> {
    let token = token.filter(|token| !token.is_empty())?;
    devices
        .iter()
        .find(|device| is_equal_secret(token, &device.token))
}

/// `app_settings`에 담긴 문자열을 기기 목록으로 되돌린다.
/// 저장값이 깨져 있으면 기기 전체를 잃는 대신 빈 목록으로 시작한다. 다시 페어링하면 복구되는 종류의 손실이다
pub fn parse_paired_devices(raw_value: Option<&str>) -> Vec<PairedDevice> {
    let Some(Value::Array(entries)) = raw_value
        .filter(|value| !value.is_empty())
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
    else {
        return Vec::new();
    };
    entries.iter().filter_map(read_paired_device).collect()
}

fn read_paired_device(value: &Value) -> Option<PairedDevice> {
    let non_empty = |key: &str| {
        value
            .get(key)?
            .as_str()
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    };
    let text = |key: &str| value.get(key)?.as_str().map(str::to_string);
    Some(PairedDevice {
        device_id: non_empty("deviceId")?,
        token: non_empty("token")?,
        device_name: text("deviceName")?,
        paired_at: text("pairedAt")?,
        notification: value.get("notification").cloned(),
    })
}

/// 기기 목록을 `app_settings`에 담을 문자열로 만든다
pub fn serialize_paired_devices(devices: &[PairedDevice]) -> String {
    serde_json::to_string(devices).expect("기기 목록은 언제나 JSON으로 바꿀 수 있다")
}

#[cfg(test)]
mod tests;
