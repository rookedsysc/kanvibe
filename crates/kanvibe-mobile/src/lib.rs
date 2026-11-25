//! 모바일 앱이 부르는 데스크톱 기능의 업무 계층(TS `mobileBridgeService`의 pane 미러 밖 부분, `mobilePairing`, 모바일 기능·AI·알림 서비스, `lanPlaintextService`).
//!
//! HTTP·WebSocket·TLS는 `kanvibe-server`가 맡고, 이 crate는 그 경로가 부르는 블로킹 메서드만 드러낸다.
//! 서버는 메서드를 `spawn_blocking` 안에서 부르고, 돌려받은 값을 TS와 같은 JSON으로 응답에 끼운다.
//! 실패는 TS 오류 이름 하나에 변형 하나가 대응하는 [`error::MobileError`]로 올린다.
//! 프로세스 환경 변수는 읽지도 바꾸지도 않는다. 홈 디렉터리·포트·푸시 중계 설정은 모두 생성 시점에 받는다.

pub mod adapters;
mod app_settings;
pub mod bridge;
pub mod clock;
pub mod devices;
pub mod error;
pub mod features;
pub mod lan_plaintext;
pub mod listeners;
pub mod notifications;
pub mod pairing;
pub mod ports;

#[cfg(test)]
mod test_support;
