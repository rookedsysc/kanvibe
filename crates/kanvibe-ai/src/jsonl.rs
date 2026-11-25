//! JSONL 기록 읽기. 빈 줄과 해석할 수 없는 줄(쓰는 중인 마지막 줄 포함)은 TS와 똑같이 건너뛴다.
//!
//! 로컬 전체 읽기도 파일을 한 번에 올리지 않고 줄 단위로 흘려 읽는다. 결과 배열은 TS의 전체 읽기와 같다.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::sync::{Arc, LazyLock, Mutex};

use serde_json::Value;

use crate::error::{AiError, AiResult};
use crate::host::{CommandOptions, SessionHost, quote_shell_argument};
use crate::javascript::{js_trim, parse_json};

/// 줄 하나를 JSON으로 읽는다. `null`과 해석할 수 없는 줄은 버린다(TS `.filter((value) => value !== null)`)
fn parse_json_line(line: &str) -> Option<Value> {
    let trimmed = js_trim(line);
    if trimmed.is_empty() {
        return None;
    }
    parse_json(trimmed).filter(|value| !value.is_null())
}

fn parse_text_lines(content: &str) -> Vec<Value> {
    content.split('\n').filter_map(parse_json_line).collect()
}

/// TS `readJsonLines`. 로컬 파일이 없으면 오류, 원격 파일이 없으면 빈 배열이다
pub fn read_json_lines(host: &SessionHost, file_path: &str) -> AiResult<Vec<Value>> {
    if host.is_remote() {
        return Ok(parse_text_lines(&host.read_text_file(file_path)?));
    }
    let file = File::open(file_path).map_err(|error| AiError::from_io(&error, file_path))?;
    let mut reader = BufReader::new(file);
    let mut values = Vec::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = reader
            .read_until(b'\n', &mut line)
            .map_err(|error| AiError::from_io(&error, file_path))?;
        if read == 0 {
            break;
        }
        values.extend(parse_json_line(&String::from_utf8_lossy(&line)));
    }
    Ok(values)
}

/// TS `readJsonLinesHead`. 해석된 값이 `max_lines`개가 되면 더 읽지 않는다.
/// 로컬은 Node readline처럼 `\r\n`, `\n`, `\r`을 모두 줄 끝으로 본다
pub fn read_json_lines_head(
    host: &SessionHost,
    file_path: &str,
    max_lines: usize,
) -> AiResult<Vec<Value>> {
    if host.is_remote() {
        let output = host.run(
            &format!("head -n {max_lines} {}", quote_shell_argument(file_path)),
            CommandOptions::default(),
        )?;
        return Ok(output
            .split('\n')
            .map(js_trim)
            .filter(|line| !line.is_empty())
            .take(max_lines)
            .filter_map(parse_json_line)
            .collect());
    }
    let file = File::open(file_path).map_err(|error| AiError::from_io(&error, file_path))?;
    read_local_head(BufReader::new(file), max_lines)
        .map_err(|error| AiError::from_io(&error, file_path))
}

fn read_local_head(mut reader: impl BufRead, max_lines: usize) -> std::io::Result<Vec<Value>> {
    let mut results = Vec::new();
    let mut line = Vec::new();
    let mut is_after_carriage_return = false;
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            break;
        }
        let mut consumed = 0;
        for &byte in buffer {
            consumed += 1;
            let is_line_feed_after_return = byte == b'\n' && is_after_carriage_return;
            is_after_carriage_return = byte == b'\r';
            if is_line_feed_after_return {
                continue;
            }
            if byte != b'\n' && byte != b'\r' {
                line.push(byte);
                continue;
            }
            results.extend(parse_json_line(&String::from_utf8_lossy(&line)));
            line.clear();
            if results.len() >= max_lines {
                return Ok(results);
            }
        }
        reader.consume(consumed);
    }
    if !line.is_empty() {
        results.extend(parse_json_line(&String::from_utf8_lossy(&line)));
    }
    Ok(results)
}

/// 최근 이벤트 60개에는 큰 tool 입력이 흔히 섞이므로, 128 KiB로 I/O를 묶되 그 창을 담을 여유를 둔다
const LOCAL_TAIL_READ_BYTES: u64 = 128 * 1024;

/// TS `readJsonLinesTail`. 로컬은 끝 128 KiB만 읽으므로 잘린 첫 줄은 해석되지 않아 버려진다
pub fn read_json_lines_tail(
    host: &SessionHost,
    file_path: &str,
    max_lines: usize,
) -> AiResult<Vec<Value>> {
    let content = if host.is_remote() {
        host.run(
            &format!("tail -n {max_lines} {}", quote_shell_argument(file_path)),
            CommandOptions::default(),
        )?
    } else {
        read_local_file_tail(file_path).map_err(|error| AiError::from_io(&error, file_path))?
    };
    let lines: Vec<&str> = content
        .split('\n')
        .map(js_trim)
        .filter(|line| !line.is_empty())
        .collect();
    let start = lines.len().saturating_sub(max_lines);
    Ok(lines[start..]
        .iter()
        .filter_map(|line| parse_json_line(line))
        .collect())
}

fn read_local_file_tail(file_path: &str) -> std::io::Result<String> {
    let mut file = File::open(file_path)?;
    let size = file.metadata()?.len();
    let read_length = size.min(LOCAL_TAIL_READ_BYTES);
    file.seek(SeekFrom::Start(size - read_length))?;
    let mut bytes = Vec::with_capacity(read_length as usize);
    file.take(read_length).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// 채팅으로 보고 있는 대화 기록만 들어오므로 적게 둔다. 줄 배열은 큰 세션에서 수십 MB가 된다
const APPENDED_JSON_LINES_MAX: usize = 8;
const CONSUMED_TAIL_BYTES: usize = 64;

/// 뒤에만 붙는 JSONL의 지금까지 읽은 줄. `log_id`가 같으면 앞쪽 줄은 지난번과 같은 값(같은 `Arc`)이다
#[derive(Debug, Clone)]
pub struct AppendedJsonLines {
    pub log_id: u64,
    pub lines: Arc<Vec<Arc<Value>>>,
}

struct AppendedLog {
    log_id: u64,
    inode: u64,
    /// 여기까지는 줄 단위로 해석해 lines에 실었다
    consumed_bytes: u64,
    /// consumed_bytes 바로 앞의 바이트. 다음에 읽을 때 같은 내용이 그 자리에 있어야 뒤에 붙은 것으로 본다
    consumed_tail: Vec<u8>,
    lines: Arc<Vec<Arc<Value>>>,
}

#[derive(Default)]
struct AppendedLogRegistry {
    /// 최근에 읽은 순서. 앞이 가장 오래됐다
    order: Vec<String>,
    logs: HashMap<String, Arc<Mutex<Option<AppendedLog>>>>,
    next_log_id: u64,
}

static APPENDED_LOGS: LazyLock<Mutex<AppendedLogRegistry>> =
    LazyLock::new(|| Mutex::new(AppendedLogRegistry::default()));

/// 파일별 잠금. 같은 파일을 겹쳐 읽으면 두 읽기가 같은 위치에서 시작해 붙은 줄을 두 번 싣기 때문에 차례로 읽는다
fn appended_log_slot(file_path: &str) -> Arc<Mutex<Option<AppendedLog>>> {
    let mut registry = APPENDED_LOGS.lock().expect("JSONL 이어 읽기 목록");
    registry.order.retain(|known| known != file_path);
    registry.order.push(file_path.to_string());
    let slot = registry
        .logs
        .entry(file_path.to_string())
        .or_default()
        .clone();
    while registry.order.len() > APPENDED_JSON_LINES_MAX {
        let oldest = registry.order.remove(0);
        registry.logs.remove(&oldest);
    }
    slot
}

fn allocate_log_id() -> u64 {
    let mut registry = APPENDED_LOGS.lock().expect("JSONL 이어 읽기 목록");
    registry.next_log_id += 1;
    registry.next_log_id
}

/// TS `readAppendedJsonLines`. 지난번에 읽은 곳 뒤로 늘어난 부분만 읽어 같은 줄 목록에 이어 붙인다.
/// 파일이 짧아졌거나, 다른 파일로 바뀌었거나, 읽었던 자리의 내용이 달라졌으면 처음부터 다시 읽어 새 목록을 만든다
pub fn read_appended_json_lines(file_path: &str) -> AiResult<AppendedJsonLines> {
    let slot = appended_log_slot(file_path);
    let mut state = slot.lock().expect("JSONL 파일 잠금");
    let to_error = |error: std::io::Error| AiError::from_io(&error, file_path);
    let mut file = File::open(file_path).map_err(to_error)?;
    let metadata = file.metadata().map_err(to_error)?;
    let appended_bytes = match state.as_ref() {
        Some(known) if known.inode == metadata.ino() && metadata.len() >= known.consumed_bytes => {
            read_bytes_after_consumed(&mut file, known, metadata.len()).map_err(to_error)?
        }
        _ => None,
    };
    if appended_bytes.is_none() {
        *state = Some(AppendedLog {
            log_id: allocate_log_id(),
            inode: metadata.ino(),
            consumed_bytes: 0,
            consumed_tail: Vec::new(),
            lines: Arc::new(Vec::new()),
        });
    }
    let unread = match appended_bytes {
        Some(bytes) => bytes,
        None => read_file_bytes(&mut file, 0, metadata.len()).map_err(to_error)?,
    };
    let log = state.as_mut().expect("방금 채웠다");
    consume_complete_json_lines(log, &unread);
    Ok(AppendedJsonLines {
        log_id: log.log_id,
        lines: log.lines.clone(),
    })
}

/// 읽었던 자리의 끝부분이 그대로면 그 뒤에 붙은 바이트를, 달라졌으면 `None`을 돌려준다
fn read_bytes_after_consumed(
    file: &mut File,
    known: &AppendedLog,
    file_size: u64,
) -> std::io::Result<Option<Vec<u8>>> {
    let tail_length = known.consumed_tail.len() as u64;
    let bytes = read_file_bytes(file, known.consumed_bytes - tail_length, file_size)?;
    let is_same_tail = bytes.get(..tail_length as usize) == Some(known.consumed_tail.as_slice());
    Ok(is_same_tail.then(|| bytes[tail_length as usize..].to_vec()))
}

fn read_file_bytes(file: &mut File, start: u64, end: u64) -> std::io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity(end.saturating_sub(start) as usize);
    file.take(end.saturating_sub(start))
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// 줄바꿈으로 끝난 줄과, 줄바꿈은 없어도 온전한 JSON인 마지막 줄까지만 싣는다. 쓰는 중인 마지막 줄은 다음 읽기로 미룬다
fn consume_complete_json_lines(log: &mut AppendedLog, unread: &[u8]) {
    let last_line_end = unread
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let last_line = parse_json(js_trim(&String::from_utf8_lossy(&unread[last_line_end..])))
        .filter(|value| !value.is_null());
    let has_complete_last_line = last_line.is_some();
    let consumed = if has_complete_last_line {
        unread
    } else {
        &unread[..last_line_end]
    };
    let complete_text = String::from_utf8_lossy(&unread[..last_line_end]);
    let lines = Arc::make_mut(&mut log.lines);
    for line in complete_text.split('\n') {
        if let Some(value) = parse_json(js_trim(line)).filter(|value| !value.is_null()) {
            lines.push(Arc::new(value));
        }
    }
    if let Some(value) = last_line {
        lines.push(Arc::new(value));
    }
    log.consumed_bytes += consumed.len() as u64;
    let mut tail_source = if consumed.len() >= CONSUMED_TAIL_BYTES {
        consumed.to_vec()
    } else {
        [log.consumed_tail.as_slice(), consumed].concat()
    };
    // 방금 읽은 큰 버퍼를 붙잡지 않도록 끝부분만 남긴다
    log.consumed_tail =
        tail_source.split_off(tail_source.len().saturating_sub(CONSUMED_TAIL_BYTES));
}

#[cfg(test)]
mod tests;
