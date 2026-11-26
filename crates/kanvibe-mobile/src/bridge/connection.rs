//! 설정 화면이 보여 주는 모바일 연결 정보.

use serde::Serialize;

use super::MobileBridge;
use crate::app_settings::open_app_settings;
use crate::error::MobileResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileConnectionSettings {
    pub mobile_tls_port: u16,
    pub is_lan_plaintext_allowed: bool,
}

impl MobileBridge {
    /// 모바일 TLS 포트와 LAN 평문 허용 여부
    pub fn read_mobile_connection_settings(&self) -> MobileResult<MobileConnectionSettings> {
        Ok(MobileConnectionSettings {
            mobile_tls_port: self.mobile_tls_port,
            is_lan_plaintext_allowed: open_app_settings(&self.database)
                .get_lan_plaintext_allowed()?,
        })
    }

    /// LAN 평문 포트를 계속 열어 둘지 저장한다. 포트는 앱 시작 때 열리므로 다음 실행부터 적용된다
    pub fn update_lan_plaintext_allowed(&self, is_allowed: bool) -> MobileResult<()> {
        Ok(open_app_settings(&self.database).set_lan_plaintext_allowed(is_allowed)?)
    }
}
