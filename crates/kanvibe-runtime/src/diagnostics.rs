//! 데스크톱 진단 로그(TS `electron/diagnostics.js`). `<userData>/logs/kanvibe-desktop.log` 한 파일에 줄 단위로 덧붙인다.
//!
//! 파일이 상한을 넘으면 백업 파일로 돌리지 않고 앞부분을 잘라 낸다. 이때 파일 전체를 메모리에 올리지 않고 남길 뒷부분만
//! 읽는다(2 GiB를 넘긴 옛 로그도 자를 수 있어야 한다). 한 줄이 너무 크면 내용 대신 잘렸다는 표시만 남긴다.
//! 로그를 쓰지 못해도 앱은 계속 돈다.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

pub const DEFAULT_DESKTOP_LOG_MAX_BYTES: u64 = 10 * 1024 * 1024;
pub const DEFAULT_DESKTOP_LOG_TRIM_TO_BYTES: u64 = 8 * 1024 * 1024;
pub const DEFAULT_DESKTOP_LOG_MAX_ENTRY_BYTES: u64 = 256 * 1024;

/// TS `resolveDesktopLogPath`
pub fn resolve_desktop_log_path(user_data_path: &Path) -> PathBuf {
    user_data_path.join("logs").join("kanvibe-desktop.log")
}

/// 파일 상한, 자를 때 남길 크기, 한 줄 상한. 남길 크기와 한 줄 상한은 파일 상한을 넘지 않는다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticsLimits {
    pub max_file_bytes: u64,
    pub trim_to_bytes: u64,
    pub max_entry_bytes: u64,
}

impl Default for DiagnosticsLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: DEFAULT_DESKTOP_LOG_MAX_BYTES,
            trim_to_bytes: DEFAULT_DESKTOP_LOG_TRIM_TO_BYTES,
            max_entry_bytes: DEFAULT_DESKTOP_LOG_MAX_ENTRY_BYTES,
        }
    }
}

type TimestampSource = Box<dyn Fn() -> String + Send + Sync>;

pub struct DesktopDiagnostics {
    log_path: PathBuf,
    limits: DiagnosticsLimits,
    timestamp: TimestampSource,
    process_meta: Value,
}

impl DesktopDiagnostics {
    /// 기본 상한, 현재 UTC 시각, 이 프로세스 정보(pid·OS·아키텍처·앱 버전)로 만든다
    pub fn new(log_path: PathBuf, app_version: &str) -> Self {
        Self::with_options(
            log_path,
            DiagnosticsLimits::default(),
            Box::new(current_iso_timestamp),
            json!({
                "pid": std::process::id(),
                "platform": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "app": app_version,
            }),
        )
    }

    pub fn with_options(
        log_path: PathBuf,
        limits: DiagnosticsLimits,
        timestamp: TimestampSource,
        process_meta: Value,
    ) -> Self {
        let max_file_bytes = limits.max_file_bytes.max(1);
        Self {
            log_path,
            limits: DiagnosticsLimits {
                max_file_bytes,
                trim_to_bytes: limits.trim_to_bytes.clamp(1, max_file_bytes),
                max_entry_bytes: limits.max_entry_bytes.clamp(1, max_file_bytes),
            },
            timestamp,
            process_meta,
        }
    }

    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    /// `[시각] 이벤트 {payload…, process}` 한 줄을 덧붙인다. 실패는 표준 오류에만 남긴다
    pub fn log(&self, event: &str, payload: Value) {
        if let Err(error) = self.append(event, payload) {
            eprintln!("[kanvibe] failed to write desktop diagnostics log: {error}");
        }
    }

    fn append(&self, event: &str, payload: Value) -> std::io::Result<()> {
        if let Some(directory) = self.log_path.parent() {
            std::fs::create_dir_all(directory)?;
        }
        let line = self.constrained_line(event, payload);
        trim_log_if_needed(&self.log_path, line.len() as u64, &self.limits)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)?;
        file.write_all(line.as_bytes())
    }

    /// TS `constrainLogLineBytes`. 상한을 넘는 줄은 원래 크기와 process 정보만 남긴다
    fn constrained_line(&self, event: &str, payload: Value) -> String {
        let timestamp = (self.timestamp)();
        let mut entry = match payload {
            Value::Object(fields) => fields,
            Value::Null => Map::new(),
            other => Map::from_iter([("value".to_string(), other)]),
        };
        entry.insert("process".to_string(), self.process_meta.clone());
        let line = build_log_line(&timestamp, event, &Value::Object(entry));
        let original_bytes = line.len() as u64;
        if original_bytes <= self.limits.max_entry_bytes {
            return line;
        }
        let truncated = json!({
            "diagnosticPayloadTruncated": true,
            "originalBytes": original_bytes,
            "maxEntryBytes": self.limits.max_entry_bytes,
            "process": self.process_meta,
        });
        build_log_line(&timestamp, event, &truncated)
    }
}

fn build_log_line(timestamp: &str, event: &str, entry: &Value) -> String {
    format!("[{timestamp}] {event} {entry}\n")
}

fn current_iso_timestamp() -> String {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as f64)
        .unwrap_or_default();
    kanvibe_ai::javascript::iso_string_from_ms(now_ms).unwrap_or_default()
}

fn file_size(log_path: &Path) -> std::io::Result<u64> {
    match std::fs::metadata(log_path) {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error),
    }
}

/// TS `trimDesktopLogIfNeeded`. 새 줄을 더해 상한을 넘으면, 더한 뒤 크기가 남길 크기가 되도록 앞을 자른다
fn trim_log_if_needed(
    log_path: &Path,
    incoming_bytes: u64,
    limits: &DiagnosticsLimits,
) -> std::io::Result<()> {
    let current_bytes = file_size(log_path)?;
    if current_bytes + incoming_bytes <= limits.max_file_bytes {
        return Ok(());
    }
    let bytes_to_keep_before_append = limits.trim_to_bytes.saturating_sub(incoming_bytes);
    trim_log_head(
        log_path,
        current_bytes.saturating_sub(bytes_to_keep_before_append),
    )
}

/// TS `trimDesktopLogHead`. 남길 뒷부분만 읽어 다시 쓴다
fn trim_log_head(log_path: &Path, bytes_to_remove: u64) -> std::io::Result<()> {
    if bytes_to_remove == 0 || !log_path.exists() {
        return Ok(());
    }
    let file_bytes = file_size(log_path)?;
    if file_bytes <= bytes_to_remove {
        return std::fs::write(log_path, b"");
    }
    let mut file = File::open(log_path)?;
    let kept_content =
        read_retained_tail(&mut file, bytes_to_remove, file_bytes - bytes_to_remove)?;
    drop(file);
    std::fs::write(log_path, kept_content)
}

/// `start`부터 `length` 바이트를 읽는다. 한 번의 읽기가 덜 채워도 끝까지 이어 읽고, 도중에 파일이 줄면 읽은 데까지 돌려준다
fn read_retained_tail<R: Read + Seek>(
    reader: &mut R,
    start: u64,
    length: u64,
) -> std::io::Result<Vec<u8>> {
    reader.seek(SeekFrom::Start(start))?;
    let mut kept_content = vec![0; length as usize];
    let mut read_bytes = 0;
    while read_bytes < kept_content.len() {
        match reader.read(&mut kept_content[read_bytes..]) {
            Ok(0) => break,
            Ok(count) => read_bytes += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    kept_content.truncate(read_bytes);
    Ok(kept_content)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn diagnostics(log_path: &Path, limits: DiagnosticsLimits) -> DesktopDiagnostics {
        DesktopDiagnostics::with_options(
            log_path.to_path_buf(),
            limits,
            Box::new(|| "2026-01-01T00:00:00.000Z".to_string()),
            json!({ "pid": 42, "platform": "test" }),
        )
    }

    fn temp_log_path() -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let log_path = resolve_desktop_log_path(directory.path());
        (directory, log_path)
    }

    fn write_legacy_log(log_path: &Path) {
        std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
        let legacy = format!(
            "legacy-start\n{}\nlegacy-tail\n",
            "legacy-middle".repeat(20)
        );
        std::fs::write(log_path, legacy).unwrap();
    }

    fn backup_path(log_path: &Path) -> PathBuf {
        PathBuf::from(format!("{}.1", log_path.display()))
    }

    // parity: TS-electron-diagnostics-abe310
    #[test]
    fn keeps_one_append_only_file_by_trimming_bytes_from_the_head() {
        let (_directory, log_path) = temp_log_path();
        let max_file_bytes = 260;
        let diagnostics = diagnostics(
            &log_path,
            DiagnosticsLimits {
                max_file_bytes,
                trim_to_bytes: 180,
                max_entry_bytes: 220,
            },
        );

        for index in 0..8 {
            diagnostics.log(
                "renderer:console-message",
                json!({ "index": index, "message": format!("entry-{index}-{}", "x".repeat(20)) }),
            );
        }

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(!content.contains("entry-0"), "{content}");
        assert!(content.contains("entry-7"), "{content}");
        assert!(std::fs::metadata(&log_path).unwrap().len() <= max_file_bytes);
        assert!(!backup_path(&log_path).exists());
    }

    // parity: TS-electron-diagnostics-f5d25a
    #[test]
    fn trims_an_oversized_legacy_log_instead_of_rotating_it() {
        let (_directory, log_path) = temp_log_path();
        write_legacy_log(&log_path);
        let max_file_bytes = 220;
        let diagnostics = diagnostics(
            &log_path,
            DiagnosticsLimits {
                max_file_bytes,
                trim_to_bytes: 150,
                max_entry_bytes: 180,
            },
        );

        diagnostics.log("main:start", json!({ "ok": true }));

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(!content.contains("legacy-start"), "{content}");
        assert!(content.contains("legacy-tail"), "{content}");
        assert!(content.contains("main:start"), "{content}");
        assert!(std::fs::metadata(&log_path).unwrap().len() <= max_file_bytes);
        assert!(!backup_path(&log_path).exists());
    }

    /// 첫 읽기만 한 바이트로 끊는 파일
    struct ShortFirstRead {
        inner: Cursor<Vec<u8>>,
        is_first_read: bool,
    }

    impl Read for ShortFirstRead {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if std::mem::take(&mut self.is_first_read) {
                return self.inner.read(&mut buffer[..1]);
            }
            self.inner.read(buffer)
        }
    }

    impl Seek for ShortFirstRead {
        fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(position)
        }
    }

    // parity: TS-electron-diagnostics-a11905
    #[test]
    fn keeps_reading_until_the_retained_tail_is_complete_when_a_read_returns_early() {
        let content = b"legacy-start\nlegacy-middle\nlegacy-tail\n".to_vec();
        let mut reader = ShortFirstRead {
            inner: Cursor::new(content.clone()),
            is_first_read: true,
        };
        let start = 13;

        let tail = read_retained_tail(&mut reader, start, content.len() as u64 - start).unwrap();

        assert_eq!(tail, b"legacy-middle\nlegacy-tail\n");
        // 실제 파일 경로로도 자른 뒤 꼬리와 새 줄이 함께 남는다
        let (_directory, log_path) = temp_log_path();
        write_legacy_log(&log_path);
        let diagnostics = diagnostics(
            &log_path,
            DiagnosticsLimits {
                max_file_bytes: 220,
                trim_to_bytes: 150,
                max_entry_bytes: 180,
            },
        );
        diagnostics.log("main:start", json!({ "ok": true }));
        let written = std::fs::read_to_string(&log_path).unwrap();
        assert!(written.contains("legacy-tail") && written.contains("main:start"));
    }

    // parity: TS-electron-diagnostics-ca3d41
    #[test]
    fn truncates_oversized_entries_before_appending_them() {
        let (_directory, log_path) = temp_log_path();
        let diagnostics = diagnostics(
            &log_path,
            DiagnosticsLimits {
                max_file_bytes: 1024,
                trim_to_bytes: 768,
                max_entry_bytes: 512,
            },
        );

        diagnostics.log(
            "renderer:console-message",
            json!({ "message": "x".repeat(5000) }),
        );

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("diagnosticPayloadTruncated"));
        assert!(content.contains("originalBytes"));
        assert!(!content.contains(&"x".repeat(1000)));
        assert!(std::fs::metadata(&log_path).unwrap().len() < 1024);
    }

    #[test]
    fn lines_carry_the_timestamp_event_payload_and_process() {
        let (_directory, log_path) = temp_log_path();
        let diagnostics = diagnostics(&log_path, DiagnosticsLimits::default());

        diagnostics.log("main:startup", json!({ "isPackaged": false }));

        let content = std::fs::read_to_string(&log_path).unwrap();
        let (prefix, entry) = content.trim_end().split_once(" {").unwrap();
        assert_eq!(prefix, "[2026-01-01T00:00:00.000Z] main:startup");
        let entry: Value = serde_json::from_str(&format!("{{{entry}")).unwrap();
        assert_eq!(
            entry,
            json!({ "isPackaged": false, "process": { "pid": 42, "platform": "test" } })
        );
    }
}
