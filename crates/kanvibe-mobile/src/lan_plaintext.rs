//! 평문 hook 포트를 LAN에 열어 둘지 정한다(TS `lanPlaintextService`).
//!
//! 터널이 안 되는 호스트와 재페어링 전의 모바일은 아직 평문으로 들어오므로, 이 설치에서 LAN 평문 요청이 일정 기간
//! 없었을 때만 루프백으로 닫는다. 사용량은 이 기기 안에서만 세며 어디로도 보내지 않는다.
//! 터널을 쓸 수 없는 Windows는 평문 외의 경로가 없어 닫지 않는다.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use kanvibe_db::{Database, DatabaseResult};

use crate::app_settings::open_app_settings;

pub const LAN_PLAINTEXT_IDLE_WINDOW_MS: i64 = 14 * 24 * 60 * 60_000;
/// 요청마다 DB에 쓰지 않도록 기록 간격을 둔다
const LAN_PLAINTEXT_RECORD_INTERVAL_MS: i64 = 60 * 60_000;
pub const LAN_BIND_HOST: &str = "0.0.0.0";
pub const LOOPBACK_BIND_HOST: &str = "127.0.0.1";
/// 인증 없이 성공하는 상태 확인 경로. 데스크탑이 원격에서 직접 부르기도 해서 실제 사용으로 보지 않는다
const HOOK_HEALTH_PATH: &str = "/api/hooks/health";

pub struct LanPlaintextUsage {
    database: Arc<Database>,
    /// 서버가 돌고 있는 호스트가 Windows인지. 호출부가 플랫폼을 판정해 넘긴다
    is_windows_host: bool,
    last_recorded_request_at: AtomicI64,
}

fn is_loopback_address(remote_address: Option<&str>) -> bool {
    matches!(
        remote_address,
        Some("127.0.0.1" | "::1" | "::ffff:127.0.0.1")
    )
}

impl LanPlaintextUsage {
    pub fn new(database: Arc<Database>, is_windows_host: bool) -> Self {
        Self {
            database,
            is_windows_host,
            last_recorded_request_at: AtomicI64::new(0),
        }
    }

    /// 사용량을 읽지 못했으면 닫을 근거가 없으므로 기존 동작(LAN)을 유지한다. 닫으면 원격 hook이 조용히 끊긴다
    pub fn resolve_hook_server_bind_host(&self, now_ms: i64) -> &'static str {
        match self.read_bind_host(now_ms) {
            Ok(bind_host) => bind_host,
            Err(error) => {
                eprintln!("[kanvibe] LAN plaintext check failed: {error}");
                LAN_BIND_HOST
            }
        }
    }

    fn read_bind_host(&self, now_ms: i64) -> DatabaseResult<&'static str> {
        let settings = open_app_settings(&self.database);
        if self.is_windows_host || settings.get_lan_plaintext_allowed()? {
            return Ok(LAN_BIND_HOST);
        }
        let observed_since = settings.get_or_start_lan_plaintext_observation(now_ms)?;
        let last_request_at = settings.get_lan_plaintext_last_request_at()?;
        let is_idle = now_ms - observed_since.max(last_request_at) >= LAN_PLAINTEXT_IDLE_WINDOW_MS;
        Ok(if is_idle {
            LOOPBACK_BIND_HOST
        } else {
            LAN_BIND_HOST
        })
    }

    /// 평문 포트에서 끝난 요청 하나를 LAN 사용으로 셀지 정한다.
    /// 실패한 요청까지 세면 같은 LAN의 스캐너가 요청만 보내도 평문 포트가 영영 닫히지 않으므로 성공한 응답만 센다
    pub fn record_lan_plaintext_response(
        &self,
        remote_address: Option<&str>,
        request_url: Option<&str>,
        status_code: u16,
        now_ms: i64,
    ) {
        if status_code >= 400 || request_url == Some(HOOK_HEALTH_PATH) {
            return;
        }
        self.record_lan_plaintext_request(remote_address, now_ms);
    }

    fn record_lan_plaintext_request(&self, remote_address: Option<&str>, now_ms: i64) {
        let last_recorded = self.last_recorded_request_at.load(Ordering::SeqCst);
        if is_loopback_address(remote_address)
            || now_ms - last_recorded < LAN_PLAINTEXT_RECORD_INTERVAL_MS
        {
            return;
        }
        self.last_recorded_request_at
            .store(now_ms, Ordering::SeqCst);
        if let Err(error) =
            open_app_settings(&self.database).set_lan_plaintext_last_request_at(now_ms)
        {
            eprintln!("[kanvibe] LAN plaintext usage record failed: {error}");
        }
    }
}

#[cfg(test)]
mod tests;
