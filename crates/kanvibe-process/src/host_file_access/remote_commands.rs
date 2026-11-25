//! 원격 파일 접근 셸 명령과 그 출력 해석. 명령 문자열은 TS와 바이트 단위로 같다.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::git_operations::resolve_path_for_shell;

use super::{HostFileError, ModifiedFile, TextFileReadResult, quote_shell_argument};

const REMOTE_FILE_RECORD_PREFIX: &str = "__KANVIBE_FILE_RECORD__";
const REMOTE_FILE_RECORD_END: &str = "__KANVIBE_FILE_RECORD_END__";

/// 경로가 있으면 `present`, 사라졌으면 `missing`을 찍는다. 가장 가까운 기존 상위 디렉터리를 탐색할 수 없으면 실패한다
pub(super) fn build_path_presence_command(target_path: &str, ssh_host: &str) -> String {
    [
        format!(
            "__kanvibe_probe={}",
            resolve_path_for_shell(target_path, Some(ssh_host), "")
        ),
        "if test -e \"$__kanvibe_probe\" || test -L \"$__kanvibe_probe\"; then".into(),
        "  printf 'present'".into(),
        "else".into(),
        "  while ! test -e \"$__kanvibe_probe\" && ! test -L \"$__kanvibe_probe\"; do".into(),
        "    __kanvibe_parent=$(dirname \"$__kanvibe_probe\")".into(),
        "    test \"$__kanvibe_parent\" != \"$__kanvibe_probe\" || exit 1".into(),
        "    __kanvibe_probe=$__kanvibe_parent".into(),
        "  done".into(),
        "  test -d \"$__kanvibe_probe\" && test -r \"$__kanvibe_probe\" && test -x \"$__kanvibe_probe\" || exit 1".into(),
        "  printf 'missing'".into(),
        "fi".into(),
    ]
    .join("\n")
}

fn encode_remote_text_file_manifest(target_paths: &[String]) -> String {
    let mut unique_paths: Vec<&str> = Vec::new();
    for target_path in target_paths {
        if !unique_paths.contains(&target_path.as_str()) {
            unique_paths.push(target_path);
        }
    }
    let encoded_paths: Vec<String> = unique_paths
        .iter()
        .map(|target_path| STANDARD.encode(target_path.as_bytes()))
        .collect();
    STANDARD.encode(encoded_paths.join("\n").as_bytes())
}

/// 파일마다 `prefix\t경로\t존재\tbase64 내용\tend` 한 줄을 찍는다. 끝 표식은 trim이 빈 내용 필드의 탭을 지우지 않게 한다
pub(super) fn build_read_text_files_command(target_paths: &[String]) -> String {
    let encoded_manifest = encode_remote_text_file_manifest(target_paths);
    let record_prefix = quote_shell_argument(REMOTE_FILE_RECORD_PREFIX);
    [
        format!(
            "__kanvibe_manifest=$(printf '%s' {} | (base64 -d 2>/dev/null || base64 -D)) || exit $?;",
            quote_shell_argument(&encoded_manifest)
        ),
        "printf \"%s\\n\" \"$__kanvibe_manifest\" | while IFS= read -r __kanvibe_encoded_file || test -n \"$__kanvibe_encoded_file\"; do".into(),
        "test -n \"$__kanvibe_encoded_file\" || continue;".into(),
        "__kanvibe_file=$(printf \"%s\" \"$__kanvibe_encoded_file\" | (base64 -d 2>/dev/null || base64 -D)) || exit $?;".into(),
        "__kanvibe_parent=$(dirname \"$__kanvibe_file\") || exit $?;".into(),
        "while ! test -e \"$__kanvibe_parent\" && ! test -L \"$__kanvibe_parent\"; do".into(),
        "__kanvibe_next_parent=$(dirname \"$__kanvibe_parent\") || exit $?;".into(),
        "[ \"$__kanvibe_parent\" != \"$__kanvibe_next_parent\" ] || exit 1;".into(),
        "__kanvibe_parent=$__kanvibe_next_parent; done;".into(),
        "test -d \"$__kanvibe_parent\" && test -x \"$__kanvibe_parent\" || exit 1;".into(),
        "if test -f \"$__kanvibe_file\"; then".into(),
        "__kanvibe_content=$(base64 -w 0 \"$__kanvibe_file\" 2>/dev/null || base64 < \"$__kanvibe_file\") || exit $?;".into(),
        format!("printf '%s\\t%s\\t1\\t' {record_prefix} \"$__kanvibe_file\";"),
        "printf \"%s\" \"$__kanvibe_content\" | tr -d \"\\n\";".into(),
        "elif test -e \"$__kanvibe_file\" || test -L \"$__kanvibe_file\"; then exit 1;".into(),
        format!("else printf '%s\\t%s\\t0\\t' {record_prefix} \"$__kanvibe_file\"; fi;"),
        format!(
            "printf '\\t%s\\n' {};",
            quote_shell_argument(REMOTE_FILE_RECORD_END)
        ),
        "done".into(),
    ]
    .join(" ")
}

/// Node `Buffer.from(value, "base64").toString("base64") === value`처럼 정규형 base64만 받는다
pub(super) fn decode_canonical_base64(encoded: &str) -> Option<Vec<u8>> {
    let decoded = STANDARD.decode(encoded).ok()?;
    (STANDARD.encode(&decoded) == encoded).then_some(decoded)
}

fn corrupted_records() -> HostFileError {
    HostFileError::Message("원격 파일 검사 결과가 손상되었습니다.".into())
}

fn parse_remote_text_file_record(
    line: &str,
    expected_paths: &BTreeSet<&str>,
    files: &BTreeMap<String, TextFileReadResult>,
) -> Result<(String, TextFileReadResult), HostFileError> {
    let fields: Vec<&str> = line.split('\t').collect();
    let [_, file_path, exists_flag, encoded_content, record_end] = fields.as_slice() else {
        return Err(corrupted_records());
    };
    let is_known_record = *record_end == REMOTE_FILE_RECORD_END
        && expected_paths.contains(file_path)
        && !files.contains_key(*file_path);
    let is_valid_flag = *exists_flag == "1" || (*exists_flag == "0" && encoded_content.is_empty());
    let decoded = decode_canonical_base64(encoded_content);
    if !is_known_record || !is_valid_flag || decoded.is_none() {
        return Err(corrupted_records());
    }
    let exists = *exists_flag == "1";
    let content = match decoded {
        Some(bytes) if exists => String::from_utf8_lossy(&bytes).into_owned(),
        _ => String::new(),
    };
    Ok((
        file_path.to_string(),
        TextFileReadResult { exists, content },
    ))
}

/// 누락되거나 손상된 레코드를 "파일 없음"으로 판정하지 않고 오류로 돌려준다
pub(super) fn parse_remote_text_files(
    output: &str,
    target_paths: &[String],
) -> Result<BTreeMap<String, TextFileReadResult>, HostFileError> {
    let expected_paths: BTreeSet<&str> = target_paths.iter().map(String::as_str).collect();
    let record_start = format!("{REMOTE_FILE_RECORD_PREFIX}\t");
    let mut files = BTreeMap::new();
    for line in output.split('\n') {
        if !line.starts_with(&record_start) {
            continue;
        }
        let (file_path, result) = parse_remote_text_file_record(line, &expected_paths, &files)?;
        files.insert(file_path, result);
    }
    if files.len() != expected_paths.len() {
        return Err(HostFileError::Message(
            "원격 파일 검사 결과가 누락되었습니다.".into(),
        ));
    }
    Ok(files)
}

fn posix_dirname(target_path: &str) -> &str {
    kanvibe_domain::project::posix_dirname(target_path)
}

pub(super) fn build_write_text_file_command(target_path: &str, content: &str) -> String {
    let encoded_content = STANDARD.encode(content.as_bytes());
    format!(
        "mkdir -p {} && printf '%s' {} | (base64 -d 2>/dev/null || base64 -D) > {}",
        quote_shell_argument(posix_dirname(target_path)),
        quote_shell_argument(&encoded_content),
        quote_shell_argument(target_path)
    )
}

pub(super) fn build_write_text_file_if_absent_command(target_path: &str, content: &str) -> String {
    let encoded_content = STANDARD.encode(content.as_bytes());
    let quoted_target_path = quote_shell_argument(target_path);
    format!(
        "mkdir -p {} && if [ -e {quoted_target_path} ]; then :; else (set -C; printf '%s' {} | (base64 -d 2>/dev/null || base64 -D) > {quoted_target_path}) || test -e {quoted_target_path}; fi",
        quote_shell_argument(posix_dirname(target_path)),
        quote_shell_argument(&encoded_content),
    )
}

pub(super) fn build_list_files_modified_within_command(
    root_path: &str,
    suffix: &str,
    within: Duration,
) -> String {
    let within_minutes = within.as_millis().div_ceil(60_000).max(1);
    let quoted_root = quote_shell_argument(root_path);
    format!(
        "test -d {quoted_root} && find {quoted_root} -type f -name {} -mmin -{within_minutes} -exec sh -c 'for f; do printf \"%s\\t%s\\n\" \"$(stat -c %Y \"$f\" 2>/dev/null || stat -f %m \"$f\" 2>/dev/null)\" \"$f\"; done' sh {{}} + || true",
        quote_shell_argument(&format!("*{suffix}"))
    )
}

/// JS `Number.parseInt(value, 10)`처럼 앞쪽 공백 뒤의 부호와 숫자만 읽는다
pub(super) fn parse_leading_integer(value: &str) -> Option<i64> {
    let trimmed = value.trim_start();
    let (sign, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let digit_length = digits
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(digits.len());
    let number: i64 = digits[..digit_length].parse().ok()?;
    Some(sign * number)
}

/// JS `line.slice(0, indexOf("\t"))`처럼 탭이 없으면 마지막 글자를 뺀 앞부분을 시각으로, 줄 전체를 경로로 본다
fn split_modified_file_line(line: &str) -> (&str, &str) {
    if let Some((seconds_value, file_path)) = line.split_once('\t') {
        return (seconds_value, file_path);
    }
    let last_character_start = line.char_indices().last().map_or(0, |(index, _)| index);
    (&line[..last_character_start], line)
}

pub(super) fn parse_modified_files(output: &str, oldest_allowed_ms: f64) -> Vec<ModifiedFile> {
    output
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let (seconds_value, file_path) = split_modified_file_line(line);
            let seconds = parse_leading_integer(seconds_value)?;
            (!file_path.is_empty()).then(|| ModifiedFile {
                file_path: file_path.to_string(),
                mtime_ms: seconds as f64 * 1000.0,
            })
        })
        .filter(|record| record.mtime_ms >= oldest_allowed_ms)
        .collect()
}
