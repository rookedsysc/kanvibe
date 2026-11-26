//! 로컬 디렉터리 목록과 수정 시각. 원격은 같은 결과를 `find`·`stat` 셸 명령으로 얻는다.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use super::ModifiedFile;

pub(super) fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64() * 1000.0)
        .unwrap_or_default()
}

fn join_entry(directory_path: &str, entry_name: &str) -> String {
    Path::new(directory_path)
        .join(entry_name)
        .to_string_lossy()
        .into_owned()
}

/// 디렉터리 바로 아래의 일반 파일 가운데 접미사가 맞는 것. 읽지 못하면 빈 목록이다
pub(super) fn read_directory_files_by_suffix(directory_path: &str, suffix: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(directory_path) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_file()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(suffix))
        .map(|name| join_entry(directory_path, &name))
        .collect()
}

/// 하위 디렉터리까지 내려가며 접미사가 맞는 항목. 링크는 따라가지 않고 이름만 본다
pub(super) fn list_files_recursively_by_suffix(root_path: &str, suffix: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root_path) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let entry_path = join_entry(root_path, &name);
        let is_directory = entry.file_type().is_ok_and(|file_type| file_type.is_dir());
        if is_directory {
            files.extend(list_files_recursively_by_suffix(&entry_path, suffix));
        } else if name.ends_with(suffix) {
            files.push(entry_path);
        }
    }
    files
}

pub(super) fn file_mtime_ms(file_path: &str) -> Option<f64> {
    let modified = std::fs::metadata(file_path).ok()?.modified().ok()?;
    let elapsed = modified.duration_since(UNIX_EPOCH).ok()?;
    Some(elapsed.as_secs_f64() * 1000.0)
}

pub(super) fn list_files_with_mtime(
    root_path: &str,
    suffix: &str,
    oldest_allowed_ms: f64,
) -> Vec<ModifiedFile> {
    list_files_recursively_by_suffix(root_path, suffix)
        .into_iter()
        .filter_map(|file_path| {
            let mtime_ms = file_mtime_ms(&file_path)?;
            Some(ModifiedFile {
                file_path,
                mtime_ms,
            })
        })
        .filter(|record| record.mtime_ms >= oldest_allowed_ms)
        .collect()
}
