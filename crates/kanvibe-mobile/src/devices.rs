//! 연결된 기기 목록과 요청 인증(TS `mobileBridgeService`의 페어링·기기·푸시 구독 부분).
//!
//! 목록은 `app_settings`의 `mobile_paired_devices` 칸 하나라 읽고-고쳐-쓰는 사이에 다른 호출이 끼면 한쪽 변경이 통째로
//! 사라진다. 두 기기가 거의 같은 순간에 연결하면 토큰 하나가 없어지고, 한쪽을 끊는 동안 다른 쪽이 연결하면 끊은 기기가
//! 되살아난다. 그래서 목록을 바꾸는 호출은 모두 [`MobileDevices::with_device_list`] 한 줄에 선다(TS `withDeviceList`).

use std::sync::{Arc, Mutex, MutexGuard};

use kanvibe_db::Database;
use kanvibe_process::request_scope::parse_bearer_token;
use serde::Serialize;
use serde_json::Value;

use crate::app_settings::open_app_settings;
use crate::clock::{EpochClock, iso_timestamp};
use crate::error::{MobileError, MobileResult};
use crate::listeners::{Listener, ListenerHandle, ListenerSet};
use crate::pairing::{
    IssuedPairingCode, PairedDevice, PairingCodes, PushPlatform, find_paired_device,
    mint_device_id, mint_device_token, parse_paired_devices, read_mobile_push_subscription,
    serialize_paired_devices,
};

const PAIRED_DEVICES_KEY: &str = "mobile_paired_devices";

pub struct MobileDevicesParts {
    pub database: Arc<Database>,
    pub clock: Arc<EpochClock>,
}

/// 연결된 기기 목록의 항목. 토큰은 대조용이라 화면에 내보내지 않는다
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDeviceSummary {
    pub device_id: String,
    pub device_name: String,
    pub paired_at: String,
}

/// 푸시 발송 대상 하나. 페어링 인증 토큰은 싣지 않는다
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushSubscriptionTarget {
    pub device_id: String,
    pub push_token: String,
    pub platform: PushPlatform,
    pub connection_id: String,
}

pub struct MobileDevices {
    database: Arc<Database>,
    clock: Arc<EpochClock>,
    pairing: PairingCodes,
    device_list_turn: Mutex<()>,
    unpaired_listeners: ListenerSet<str>,
}

/// 목록을 바꾼 뒤 저장할지
enum DeviceListWrite {
    Save,
    Keep,
}

fn lock(turn: &Mutex<()>) -> MutexGuard<'_, ()> {
    turn.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl MobileDevices {
    pub fn new(parts: MobileDevicesParts) -> Self {
        Self {
            database: parts.database,
            clock: parts.clock,
            pairing: PairingCodes::default(),
            device_list_turn: Mutex::new(()),
            unpaired_listeners: ListenerSet::default(),
        }
    }

    /// 화면에 띄울 페어링 코드를 발급한다
    pub fn start_mobile_pairing(&self) -> IssuedPairingCode {
        self.pairing.issue((self.clock)())
    }

    /// 페어링 화면을 닫는다
    pub fn stop_mobile_pairing(&self) {
        self.pairing.revoke();
    }

    /// 화면에 떠 있는 코드. 데스크탑 화면이 다시 그릴 때 쓴다
    pub fn read_mobile_pairing_code(&self) -> Option<String> {
        self.pairing.read_pending((self.clock)())
    }

    fn read_paired_devices(&self) -> MobileResult<Vec<PairedDevice>> {
        let stored = open_app_settings(&self.database).get_app_setting(PAIRED_DEVICES_KEY)?;
        Ok(parse_paired_devices(stored.as_deref()))
    }

    fn with_device_list<T>(
        &self,
        mutate: impl FnOnce(&mut Vec<PairedDevice>) -> (T, DeviceListWrite),
    ) -> MobileResult<T> {
        let _turn = lock(&self.device_list_turn);
        let mut devices = self.read_paired_devices()?;
        let (result, write) = mutate(&mut devices);
        if let DeviceListWrite::Save = write {
            open_app_settings(&self.database)
                .set_app_setting(PAIRED_DEVICES_KEY, &serialize_paired_devices(&devices))?;
        }
        Ok(result)
    }

    /// 코드를 확인하고 기기 토큰을 발급한다. 토큰은 만료시키지 않는다
    pub fn pair_mobile_device(
        &self,
        submitted_code: &str,
        device_name: &str,
    ) -> MobileResult<Option<String>> {
        if !self.pairing.redeem(submitted_code, (self.clock)()) {
            return Ok(None);
        }
        let token = mint_device_token();
        let paired_at = iso_timestamp((self.clock)());
        self.with_device_list(|devices| {
            devices.push(PairedDevice {
                device_id: mint_device_id(),
                token: token.clone(),
                device_name: device_name.to_string(),
                paired_at,
                notification: None,
            });
            ((), DeviceListWrite::Save)
        })?;
        Ok(Some(token))
    }

    pub fn list_paired_mobile_devices(&self) -> MobileResult<Vec<PairedDeviceSummary>> {
        Ok(self
            .read_paired_devices()?
            .into_iter()
            .map(|device| PairedDeviceSummary {
                device_id: device.device_id,
                device_name: device.device_name,
                paired_at: device.paired_at,
            })
            .collect())
    }

    /// 요청이 증명한 기기 하나의 푸시 구독을 등록하거나 회전된 토큰으로 바꾼다
    pub fn update_mobile_notification_subscription(
        &self,
        authorization_header: Option<&str>,
        subscription: &Value,
    ) -> MobileResult<bool> {
        let Some(subscription) = read_mobile_push_subscription(subscription) else {
            return Err(MobileError::task_input("알림 구독 값이 올바르지 않습니다"));
        };
        let stored = serde_json::to_value(&subscription).map_err(MobileError::internal)?;
        let token = parse_bearer_token(authorization_header);
        self.with_device_list(|devices| {
            let Some(device_id) = find_paired_device(devices, token).map(|d| d.device_id.clone())
            else {
                return (false, DeviceListWrite::Keep);
            };
            for device in devices.iter_mut().filter(|d| d.device_id == device_id) {
                device.notification = Some(stored.clone());
            }
            (true, DeviceListWrite::Save)
        })
    }

    /// 알림만 끄는 동작은 페어링과 터미널 인증을 유지한다
    pub fn delete_mobile_notification_subscription(
        &self,
        authorization_header: Option<&str>,
    ) -> MobileResult<bool> {
        let token = parse_bearer_token(authorization_header);
        self.with_device_list(|devices| {
            let Some(device_id) = find_paired_device(devices, token).map(|d| d.device_id.clone())
            else {
                return (false, DeviceListWrite::Keep);
            };
            for device in devices.iter_mut().filter(|d| d.device_id == device_id) {
                device.notification = None;
            }
            (true, DeviceListWrite::Save)
        })
    }

    /// 푸시 발송 경계에는 페어링 인증 토큰을 넘기지 않는다
    pub fn list_mobile_push_subscriptions(&self) -> MobileResult<Vec<PushSubscriptionTarget>> {
        Ok(self
            .read_paired_devices()?
            .into_iter()
            .filter_map(|device| {
                let subscription = read_mobile_push_subscription(device.notification.as_ref()?)?;
                Some(PushSubscriptionTarget {
                    device_id: device.device_id,
                    push_token: subscription.push_token,
                    platform: subscription.platform,
                    connection_id: subscription.connection_id,
                })
            })
            .collect())
    }

    /// 늦은 만료 응답이 새로 등록된 토큰을 지우지 않도록 기기와 발송했던 토큰을 함께 대조한다
    pub fn remove_unregistered_mobile_push_subscription(
        &self,
        device_id: &str,
        push_token: &str,
    ) -> MobileResult<()> {
        self.with_device_list(|devices| {
            let device = devices
                .iter_mut()
                .find(|candidate| candidate.device_id == device_id);
            let Some(device) = device.filter(|device| {
                stored_push_token(device.notification.as_ref()) == Some(push_token)
            }) else {
                return ((), DeviceListWrite::Keep);
            };
            device.notification = None;
            ((), DeviceListWrite::Save)
        })
    }

    /// 설정 화면이 부르는 연결 해제. 이미 없던 기기까지 알리면 듣는 쪽이 남의 소켓을 닫을 근거로 삼으므로 지운 경우에만 알린다
    pub fn unpair_mobile_device(&self, device_id: &str) -> MobileResult<()> {
        let was_removed = self.with_device_list(|devices| {
            let count_before = devices.len();
            devices.retain(|device| device.device_id != device_id);
            (devices.len() != count_before, DeviceListWrite::Save)
        })?;
        if was_removed {
            self.notify_mobile_device_unpaired(device_id);
        }
        Ok(())
    }

    /// 요청에 실린 토큰의 주인만 지운다. 모바일의 "연결 끊기"가 부르는 경로다.
    /// 토큰 하나로 남의 기기까지 끊을 수 있으면 없던 권한이 새로 생기므로 지우는 대상은 요청이 증명한 기기 하나뿐이다
    pub fn unpair_mobile_device_by_token(
        &self,
        authorization_header: Option<&str>,
    ) -> MobileResult<bool> {
        let token = parse_bearer_token(authorization_header);
        let removed_device_id = self.with_device_list(|devices| {
            let Some(device_id) = find_paired_device(devices, token).map(|d| d.device_id.clone())
            else {
                return (None, DeviceListWrite::Keep);
            };
            devices.retain(|candidate| candidate.device_id != device_id);
            (Some(device_id), DeviceListWrite::Save)
        })?;
        if let Some(device_id) = &removed_device_id {
            self.notify_mobile_device_unpaired(device_id);
        }
        Ok(removed_device_id.is_some())
    }

    /// 기기 연결이 끊겼다는 것을 알려 달라고 등록한다. 소켓·로그인·원격 터미널 권한을 가진 쪽이 그 기기 것을 닫는다
    pub fn on_mobile_device_unpaired(&self, listener: Listener<str>) -> ListenerHandle {
        self.unpaired_listeners.add(listener)
    }

    /// 알림은 목록 줄 밖에서 돈다. 듣는 쪽이 오래 걸려도 다음 목록 변경이 그만큼 밀리면 안 된다
    fn notify_mobile_device_unpaired(&self, device_id: &str) {
        self.unpaired_listeners
            .emit(device_id, "Mobile device unpaired delivery failed");
    }

    /// 요청이 연결된 기기에서 온 것이면 그 기기의 식별자, 아니면 `None`
    pub fn authorize_mobile_request(
        &self,
        authorization_header: Option<&str>,
    ) -> MobileResult<Option<String>> {
        let devices = self.read_paired_devices()?;
        Ok(
            find_paired_device(&devices, parse_bearer_token(authorization_header))
                .map(|device| device.device_id.clone()),
        )
    }
}

fn stored_push_token(notification: Option<&Value>) -> Option<&str> {
    notification?.get("pushToken")?.as_str()
}

#[cfg(test)]
mod tests;
