//! 실행 방식에 따라 hook 포트와 앱 데이터 디렉터리를 고른다(TS `electron/runtimeEnvironment.js`).
//!
//! 사용자 프로젝트에 설치된 hook이 부르는 포트라서 설치된 앱만 9736을 쓴다. 데이터 디렉터리를 고르는 기준과 같은 기준으로
//! 포트도 골라, 데이터가 다른 실행끼리는 포트도 겹치지 않게 한다. 값은 모두 호출자가 시작할 때 한 번 읽어 넘긴다.

use std::path::{Path, PathBuf};

pub const PACKAGED_HOOK_SERVER_PORT: u16 = 9736;
pub const DEV_HOOK_SERVER_PORT: u16 = 19736;
/// 수동 QA 실행. 설치된 앱과 같은 포트를 쓰면 둘 중 하나만 hook을 받는다
pub const MANUAL_QA_HOOK_SERVER_PORT: u16 = 29736;
pub const HOOK_SERVER_PORT_SWITCH: &str = "kanvibe-hook-port";

/// 실행 방식. 프로세스 환경과 명령줄은 호출자가 읽어 채운다
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchMode {
    /// 설치된 앱(패키지)으로 실행했는지
    pub is_packaged: bool,
    /// `--qa`로 띄운 수동 QA인지
    pub is_manual_qa: bool,
    /// 소스 개발 실행인지(TS `KANVIBE_RENDERER_URL`)
    pub is_dev_run: bool,
    /// 자동 QA인지(`KANVIBE_QA_MODE=1`)
    pub is_automated_qa: bool,
    /// `KANVIBE_APP_DATA_DIR` 원문
    pub requested_app_data_dir: Option<String>,
    /// `--kanvibe-hook-port` 원문
    pub requested_hook_port: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LaunchError {
    #[error("Invalid --kanvibe-hook-port: {0}")]
    InvalidHookPort(String),
    #[error("Automated QA requires an isolated KANVIBE_APP_DATA_DIR")]
    MissingQaDataDirectory,
}

pub fn resolve_hook_server_port(mode: &LaunchMode) -> Result<u16, LaunchError> {
    if let Some(requested) = mode
        .requested_hook_port
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return parse_requested_hook_port(requested);
    }
    if mode.is_dev_run && !mode.is_manual_qa {
        return Ok(DEV_HOOK_SERVER_PORT);
    }
    if mode.is_packaged && !mode.is_manual_qa {
        return Ok(PACKAGED_HOOK_SERVER_PORT);
    }
    Ok(MANUAL_QA_HOOK_SERVER_PORT)
}

/// JS `Number(value)`가 1~65535 정수일 때만 받는다
fn parse_requested_hook_port(requested: &str) -> Result<u16, LaunchError> {
    let invalid = || LaunchError::InvalidHookPort(requested.to_string());
    let trimmed = kanvibe_ai::javascript::js_trim(requested);
    let port: f64 = trimmed.parse().map_err(|_| invalid())?;
    let is_valid = port.fract() == 0.0 && port > 0.0 && port <= 65535.0;
    is_valid.then_some(port as u16).ok_or_else(invalid)
}

/// 원격 daemon·agent namespace(TS `startHookServer`의 `remoteHostNamespace`). 설치된 앱은 `prod`, 소스 개발 실행은
/// `dev`, 나머지(QA 실행)는 데이터 디렉터리마다 따로 뜨도록 `qa-<sha256(userData) 앞 16자>`다
pub fn resolve_remote_host_namespace(mode: &LaunchMode, app_data_directory: &Path) -> String {
    if mode.is_packaged && !mode.is_manual_qa {
        return "prod".to_string();
    }
    if mode.is_dev_run && !mode.is_manual_qa {
        return "dev".to_string();
    }
    qa_remote_host_namespace(app_data_directory)
}

/// TS `qa-${sha256(userData).slice(0, 16)}`
pub fn qa_remote_host_namespace(app_data_directory: &Path) -> String {
    use sha2::Digest;
    let digest = hex::encode(sha2::Sha256::digest(
        app_data_directory.to_string_lossy().as_bytes(),
    ));
    format!("qa-{}", &digest[..16])
}

/// 소스 개발 실행과 자동 QA 실행에서만 여는 `/api/mobile/dev/pairing-code`(TS `IS_DEV_PAIRING_ROUTE_ENABLED`).
/// TS 자동 QA(모바일 E2E fixture 포함)는 늘 소스(`KANVIBE_RENDERER_URL`)로 띄웠으므로 그 실행에도 이 경로가 열려 있었다
pub fn is_dev_pairing_route_enabled(mode: &LaunchMode) -> bool {
    (mode.is_dev_run || mode.is_automated_qa) && !mode.is_packaged
}

/// 공백뿐인 값은 없는 것으로 본다
pub fn requested_app_data_directory(mode: &LaunchMode) -> Option<String> {
    let trimmed = mode.requested_app_data_dir.as_deref()?.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// 앱 데이터 디렉터리. `platform_app_data`는 OS 앱 데이터 루트, `installed_user_data`는 설치된 앱의 기본 위치다
pub fn resolve_app_data_directory(
    mode: &LaunchMode,
    platform_app_data: &Path,
    installed_user_data: &Path,
) -> Result<PathBuf, LaunchError> {
    if let Some(requested) = requested_app_data_directory(mode) {
        return Ok(PathBuf::from(requested));
    }
    if mode.is_automated_qa {
        return Err(LaunchError::MissingQaDataDirectory);
    }
    if !mode.is_packaged || mode.is_manual_qa {
        let directory = if mode.is_dev_run && !mode.is_manual_qa {
            "kanvibe-dev"
        } else {
            "kanvibe-qa"
        };
        return Ok(platform_app_data.join(directory));
    }
    Ok(installed_user_data.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP_DATA: &str = "/home/user/.config";
    const USER_DATA: &str = "/home/user/.config/kanvibe";

    fn resolve(mode: &LaunchMode) -> Result<PathBuf, LaunchError> {
        resolve_app_data_directory(mode, Path::new(APP_DATA), Path::new(USER_DATA))
    }

    fn dev() -> LaunchMode {
        LaunchMode {
            is_dev_run: true,
            ..LaunchMode::default()
        }
    }

    #[test]
    fn uses_the_requested_app_data_directory() {
        let mode = LaunchMode {
            requested_app_data_dir: Some("/tmp/kanvibe-qa-run/app-data".into()),
            ..LaunchMode::default()
        };
        assert_eq!(
            resolve(&mode),
            Ok(PathBuf::from("/tmp/kanvibe-qa-run/app-data"))
        );
    }

    #[test]
    fn keeps_the_installed_app_user_data_path() {
        let mode = LaunchMode {
            is_packaged: true,
            ..LaunchMode::default()
        };
        assert_eq!(resolve(&mode), Ok(PathBuf::from(USER_DATA)));
    }

    #[test]
    fn selects_persistent_data_by_launch_mode() {
        let cases = [
            (dev(), "kanvibe-dev"),
            (LaunchMode::default(), "kanvibe-qa"),
            (
                LaunchMode {
                    is_packaged: true,
                    is_manual_qa: true,
                    ..LaunchMode::default()
                },
                "kanvibe-qa",
            ),
            (
                LaunchMode {
                    is_manual_qa: true,
                    ..dev()
                },
                "kanvibe-qa",
            ),
        ];
        for (mode, directory) in cases {
            assert_eq!(resolve(&mode), Ok(Path::new(APP_DATA).join(directory)));
        }
    }

    #[test]
    fn manual_qa_restarts_reuse_the_same_directory() {
        assert_eq!(
            resolve(&LaunchMode::default()),
            resolve(&LaunchMode::default())
        );
    }

    #[test]
    fn blank_data_directory_override_counts_as_absent() {
        let mode = LaunchMode {
            requested_app_data_dir: Some("  ".into()),
            ..LaunchMode::default()
        };
        assert_eq!(resolve(&mode), Ok(Path::new(APP_DATA).join("kanvibe-qa")));
    }

    #[test]
    fn automated_qa_requires_a_fixture_directory() {
        let mode = LaunchMode {
            is_automated_qa: true,
            ..LaunchMode::default()
        };
        let error = resolve(&mode).unwrap_err();
        assert!(error.to_string().contains("KANVIBE_APP_DATA_DIR"));
    }

    #[test]
    fn automated_qa_uses_the_trimmed_fixture_directory() {
        let mode = LaunchMode {
            is_automated_qa: true,
            requested_app_data_dir: Some(" /tmp/kanvibe-fixture ".into()),
            ..LaunchMode::default()
        };
        assert_eq!(resolve(&mode), Ok(PathBuf::from("/tmp/kanvibe-fixture")));
    }

    #[test]
    fn each_launch_kind_gets_its_own_hook_port() {
        let cases = [
            (
                LaunchMode {
                    is_packaged: true,
                    ..LaunchMode::default()
                },
                9736,
            ),
            (dev(), 19736),
            (LaunchMode::default(), 29736),
            (
                LaunchMode {
                    is_packaged: true,
                    is_manual_qa: true,
                    ..LaunchMode::default()
                },
                29736,
            ),
            (
                LaunchMode {
                    is_manual_qa: true,
                    ..dev()
                },
                29736,
            ),
        ];
        for (mode, port) in cases {
            assert_eq!(resolve_hook_server_port(&mode), Ok(port));
        }
    }

    #[test]
    fn automated_qa_launcher_port_wins() {
        let mode = LaunchMode {
            is_automated_qa: true,
            requested_hook_port: Some("41234".into()),
            ..dev()
        };
        assert_eq!(resolve_hook_server_port(&mode), Ok(41234));
    }

    #[test]
    fn remote_namespace_separates_installed_dev_and_qa_runs() {
        let packaged = LaunchMode {
            is_packaged: true,
            ..LaunchMode::default()
        };
        let data = Path::new("/tmp/kanvibe-qa-data");
        assert_eq!(resolve_remote_host_namespace(&packaged, data), "prod");
        assert_eq!(resolve_remote_host_namespace(&dev(), data), "dev");
        let qa = resolve_remote_host_namespace(&LaunchMode::default(), data);
        assert_eq!(qa, qa_remote_host_namespace(data));
        assert!(qa.starts_with("qa-") && qa.len() == 19, "{qa}");
        assert!(is_dev_pairing_route_enabled(&dev()));
        assert!(!is_dev_pairing_route_enabled(&packaged));
    }

    #[test]
    fn automated_qa_opens_the_dev_pairing_route_like_the_ts_source_launch() {
        let automated_qa = LaunchMode {
            is_automated_qa: true,
            requested_app_data_dir: Some("/tmp/kanvibe-qa-data".into()),
            ..LaunchMode::default()
        };
        let manual_qa = LaunchMode {
            is_manual_qa: true,
            ..LaunchMode::default()
        };

        assert!(is_dev_pairing_route_enabled(&automated_qa));
        assert!(!is_dev_pairing_route_enabled(&manual_qa));
        assert!(!is_dev_pairing_route_enabled(&LaunchMode::default()));
    }

    #[test]
    fn invalid_requested_hook_ports_are_refused() {
        for requested in ["abc", "0", "65536", "1.5"] {
            let mode = LaunchMode {
                requested_hook_port: Some(requested.into()),
                ..LaunchMode::default()
            };
            let error = resolve_hook_server_port(&mode).unwrap_err();
            assert!(error.to_string().contains("--kanvibe-hook-port"));
        }
    }
}
