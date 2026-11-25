//! AI 계정 관리와 사용량 서비스. TS main 서비스(`aiUsageService`, `aiAccountService`)와 로그인 브리지
//! (`aiAccountLoginBridge`, `mobileAiLoginBridge`)에서 UI가 아닌 부분을 옮긴다. 로그인 PTY는
//! [`login_terminal::LoginTerminalSpawner`]로 받아 `kanvibe-terminal`이 구현한다.

pub mod account_service;
pub mod desktop_login_bridge;
pub mod login_terminal;
pub mod mobile_login_bridge;
pub mod settings_store;
pub mod usage_service;
