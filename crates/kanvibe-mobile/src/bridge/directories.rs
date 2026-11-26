//! 연결된 PC의 폴더 탐색과 프로젝트 스캔. 기존 데스크탑 조회는 find 오류를 빈 목록으로 바꾸므로,
//! 모바일은 폴더가 실제로 있고 읽을 수 있는지 먼저 확인해 "없는 폴더"를 "빈 폴더"로 숨기지 않는다.

use std::ffi::CString;
use std::io::ErrorKind;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use kanvibe_services::project::ScanResult;

use super::MobileBridge;
use crate::error::{MobileError, MobileResult};

fn is_control_character(character: char) -> bool {
    matches!(character, '\u{0}'..='\u{1f}' | '\u{7f}')
}

/// Node `path.normalize`처럼 `.`·`..`·빈 조각을 정리한다. 끝의 `/`는 남긴다
pub(crate) fn normalize_posix_path(path: &str) -> String {
    let is_absolute = path.starts_with('/');
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." if segments.last().is_some_and(|last| *last != "..") => {
                segments.pop();
            }
            ".." if is_absolute => {}
            _ => segments.push(segment),
        }
    }
    let joined = segments.join("/");
    let trailing = if path.ends_with('/') && !joined.is_empty() {
        "/"
    } else {
        ""
    };
    match (is_absolute, joined.is_empty()) {
        (true, _) => format!("/{joined}{trailing}"),
        (false, true) => ".".to_string(),
        (false, false) => format!("{joined}{trailing}"),
    }
}

/// `R_OK | X_OK`. 목록을 읽고 하위 폴더로 들어갈 수 있어야 탐색이 된다
fn check_directory_access(path: &str) -> std::io::Result<()> {
    let path = CString::new(Path::new(path).as_os_str().as_bytes())
        .map_err(|error| std::io::Error::new(ErrorKind::InvalidInput, error))?;
    // SAFETY: 널 문자로 끝나는 경로를 넘기고, access는 그 메모리를 읽기만 한다
    let result = unsafe { libc::access(path.as_ptr(), libc::R_OK | libc::X_OK) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn to_directory_error(error: std::io::Error) -> MobileError {
    match error.kind() {
        ErrorKind::NotFound | ErrorKind::NotADirectory => {
            MobileError::task_input("폴더를 찾을 수 없습니다")
        }
        ErrorKind::PermissionDenied => MobileError::task_input("이 폴더에 접근할 권한이 없습니다"),
        _ => MobileError::internal(error),
    }
}

fn verify_readable_directory(path: &str) -> MobileResult<()> {
    let metadata = std::fs::metadata(path).map_err(to_directory_error)?;
    if !metadata.is_dir() {
        return Err(MobileError::task_input("폴더를 선택해 주세요"));
    }
    check_directory_access(path).map_err(to_directory_error)
}

impl MobileBridge {
    fn resolve_mobile_project_directory(&self, submitted_path: &str) -> MobileResult<String> {
        if submitted_path.is_empty() || submitted_path.chars().any(is_control_character) {
            return Err(MobileError::task_input("올바른 폴더 경로를 입력해 주세요"));
        }
        let expanded_path = if submitted_path == "~" || submitted_path.starts_with("~/") {
            // Node `path.join(home, "")`은 빈 조각을 버리므로 `~` 하나는 홈 그대로다
            let home = self.home_directory.to_string_lossy();
            let rest = &submitted_path[1..];
            let joined = if rest.is_empty() {
                home.to_string()
            } else {
                format!("{home}/{rest}")
            };
            normalize_posix_path(&joined)
        } else {
            submitted_path.to_string()
        };
        if !expanded_path.starts_with('/') {
            return Err(MobileError::task_input(
                "절대 경로나 ~/로 시작하는 폴더를 선택해 주세요",
            ));
        }
        verify_readable_directory(&expanded_path)?;
        Ok(expanded_path)
    }

    pub fn get_mobile_directories(&self, parent_path: &str) -> MobileResult<Vec<String>> {
        let directory = self.resolve_mobile_project_directory(parent_path)?;
        Ok(self.projects.list_subdirectories(&directory, None))
    }

    pub fn scan_mobile_projects(&self, root_path: &str) -> MobileResult<ScanResult> {
        let directory = self.resolve_mobile_project_directory(root_path)?;
        Ok(self.projects.scan_and_register_projects(&directory, None)?)
    }
}
