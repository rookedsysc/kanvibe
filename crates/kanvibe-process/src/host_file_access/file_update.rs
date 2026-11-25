//! 잠금을 잡고 읽기·수정·교체하는 파일 갱신. 로컬과 원격이 같은 디스크 잠금 규약을 쓴다.
//! 잠금은 `<file>.lock` 디렉터리 하나와 그 안의 `owner-<pid>-<uuid>-<hostname hex>` 디렉터리 하나다.
//! 그래서 같은 저장소를 로컬로 여는 KanVibe와 SSH로 여는 다른 기기의 KanVibe가 서로를 기다린다.
//! 같은 프로세스 안의 갱신은 디스크 잠금 앞에서 먼저 줄을 세워, 잠금 재시도 경합으로 시간이 초과되지 않게 한다.

use std::collections::HashSet;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use regex::Regex;

use super::remote_commands::decode_canonical_base64;
use super::{
    HostFileError, REMOTE_TEXT_FILE_MAX_BUFFER_BYTES, TextFileUpdate, is_not_found,
    quote_shell_argument, read_local_text,
};
use crate::exec::{ExecGitError, ExecGitOptions, RemoteStdinResponder, ShellCommandRunner};

const FILE_UPDATE_LOCK_ATTEMPTS: usize = 200;
const FILE_UPDATE_LOCK_RETRY: Duration = Duration::from_millis(50);
const FILE_UPDATE_OWNER_INITIALIZATION_ATTEMPTS: usize = 10;
const REMOTE_FILE_UPDATE_READY: &str = "__KANVIBE_FILE_UPDATE_READY__";
const REMOTE_FILE_UPDATE_DONE: &str = "__KANVIBE_FILE_UPDATED__";
const REMOTE_FILE_UPDATE_BUSY: &str = "__KANVIBE_FILE_UPDATE_BUSY__";
const REMOTE_FILE_UPDATE_UNOWNED: &str = "__KANVIBE_FILE_UPDATE_UNOWNED__";

static LOCK_OWNER_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("^owner-([1-9][0-9]*)-[a-zA-Z0-9-]+-([a-f0-9]+)$").expect("유효한 정규식")
});

/// 같은 프로세스 안에서 (호스트, 경로)마다 갱신을 하나씩만 디스크 잠금으로 보낸다
struct InProcessFileLockRegistry {
    busy_keys: Mutex<HashSet<String>>,
    released: Condvar,
}

static IN_PROCESS_FILE_LOCKS: LazyLock<InProcessFileLockRegistry> =
    LazyLock::new(|| InProcessFileLockRegistry {
        busy_keys: Mutex::new(HashSet::new()),
        released: Condvar::new(),
    });

pub(super) struct InProcessFileLock {
    key: String,
}

impl InProcessFileLock {
    pub(super) fn acquire(ssh_host: Option<&str>, target_path: &str) -> Self {
        let key = format!("{}\u{0}{target_path}", ssh_host.unwrap_or_default());
        let registry = &*IN_PROCESS_FILE_LOCKS;
        let mut busy_keys = registry.busy_keys.lock().unwrap();
        while busy_keys.contains(&key) {
            busy_keys = registry.released.wait(busy_keys).unwrap();
        }
        busy_keys.insert(key.clone());
        Self { key }
    }
}

impl Drop for InProcessFileLock {
    fn drop(&mut self) {
        let registry = &*IN_PROCESS_FILE_LOCKS;
        registry.busy_keys.lock().unwrap().remove(&self.key);
        registry.released.notify_all();
    }
}

fn lock_wait_timeout(lock_path: &str) -> HostFileError {
    HostFileError::Message(format!("파일 잠금 대기 시간이 초과되었습니다: {lock_path}"))
}

fn unowned_lock(lock_path: &str) -> HostFileError {
    HostFileError::Message(format!(
        "소유자를 확인할 수 없는 파일 잠금입니다. 사용 중인 작업이 없는지 확인한 뒤 잠금을 백업 이동하세요: {lock_path}"
    ))
}

/// Node `os.hostname()`을 hex로 바꾼 값. 원격 셸은 `uname -n | od`로 같은 값을 만든다
fn local_hostname_hex() -> String {
    let mut buffer = [0_u8; 256];
    // SAFETY: 버퍼 길이를 넘지 않게 쓰고, 끝의 NUL 앞까지만 읽는다
    let result = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) };
    if result != 0 {
        return String::new();
    }
    let length = buffer
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(buffer.len());
    hex::encode(&buffer[..length])
}

fn is_process_gone(process_id: i32) -> bool {
    // SAFETY: 신호 0은 프로세스 존재만 확인하고 아무 신호도 보내지 않는다
    let result = unsafe { libc::kill(process_id, 0) };
    result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

fn ignore_not_found(result: std::io::Result<()>, path: &Path) -> Result<(), HostFileError> {
    match result {
        Err(error) if !is_not_found(&error) => {
            Err(HostFileError::io(&path.to_string_lossy(), error))
        }
        _ => Ok(()),
    }
}

/// owner 디렉터리를 지운 다음에만 잠금 디렉터리를 지운다. 이미 다른 owner로 바뀐 잠금은 건드리지 않는다
fn remove_lock_owner(owner_path: &Path, lock_path: &Path) -> Result<(), HostFileError> {
    match std::fs::remove_dir(owner_path) {
        Ok(()) => ignore_not_found(std::fs::remove_dir(lock_path), lock_path),
        Err(error) => ignore_not_found(Err(error), owner_path),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum LockAttempt {
    Acquired,
    Busy,
    Unowned,
}

/// 남의 잠금을 살펴 이 호스트에서 이미 끝난 프로세스의 잠금이면 치우고 다시 시도하게 한다
fn inspect_existing_lock(lock_path: &Path, host_hex: &str) -> Result<LockAttempt, HostFileError> {
    let lock_label = lock_path.to_string_lossy();
    let metadata = match std::fs::symlink_metadata(lock_path) {
        Ok(metadata) => metadata,
        Err(error) if is_not_found(&error) => return Ok(LockAttempt::Busy),
        Err(error) => return Err(HostFileError::io(&lock_label, error)),
    };
    if !metadata.is_dir() {
        return Err(HostFileError::Message(format!(
            "파일 잠금 경로가 디렉터리가 아닙니다: {lock_label}"
        )));
    }
    let owners: Vec<String> = match std::fs::read_dir(lock_path) {
        Ok(entries) => entries
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect(),
        Err(error) if is_not_found(&error) => Vec::new(),
        Err(error) => return Err(HostFileError::io(&lock_label, error)),
    };
    let owner = if owners.len() == 1 {
        owners[0].as_str()
    } else {
        ""
    };
    let Some(captures) = LOCK_OWNER_PATTERN.captures(owner) else {
        return Ok(LockAttempt::Unowned);
    };
    if &captures[2] != host_hex {
        return Ok(LockAttempt::Unowned);
    }
    let is_stale = captures[1].parse::<i32>().is_ok_and(is_process_gone);
    if is_stale {
        remove_lock_owner(&lock_path.join(owner), lock_path)?;
    }
    Ok(LockAttempt::Busy)
}

fn try_acquire_local_file_lock(
    lock_path: &Path,
    owner_path: &Path,
    host_hex: &str,
) -> Result<LockAttempt, HostFileError> {
    let created = std::fs::DirBuilder::new().mode(0o700).create(lock_path);
    match created {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return inspect_existing_lock(lock_path, host_hex);
        }
        Err(error) => return Err(HostFileError::io(&lock_path.to_string_lossy(), error)),
    }
    if let Err(error) = std::fs::create_dir(owner_path) {
        let _ = std::fs::remove_dir(lock_path);
        return Err(HostFileError::io(&owner_path.to_string_lossy(), error));
    }
    Ok(LockAttempt::Acquired)
}

/// 소유자 없는 잠금은 잠깐 생기는 초기화·해제 구간일 수 있으므로 연달아 관측될 때만 보고한다
fn acquire_local_file_lock(lock_path: &Path, owner_path: &Path) -> Result<(), HostFileError> {
    let host_hex = local_hostname_hex();
    let lock_label = lock_path.to_string_lossy();
    let mut unowned_observations = 0;
    for attempt in 0..FILE_UPDATE_LOCK_ATTEMPTS {
        match try_acquire_local_file_lock(lock_path, owner_path, &host_hex)? {
            LockAttempt::Acquired => return Ok(()),
            LockAttempt::Unowned => unowned_observations += 1,
            LockAttempt::Busy => unowned_observations = 0,
        }
        if unowned_observations >= FILE_UPDATE_OWNER_INITIALIZATION_ATTEMPTS {
            return Err(unowned_lock(&lock_label));
        }
        if attempt + 1 < FILE_UPDATE_LOCK_ATTEMPTS {
            std::thread::sleep(FILE_UPDATE_LOCK_RETRY);
        }
    }
    Err(lock_wait_timeout(&lock_label))
}

/// 내용을 담는 임시 파일을 처음부터 `mode` 권한으로 만든다. 교체 전후 어느 순간에도 넓은 권한으로 열리지 않는다
pub(super) fn create_exclusive_file(
    temporary_path: &str,
    content: &str,
    mode: Option<u32>,
) -> Result<(), HostFileError> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode.unwrap_or(0o666))
        .open(temporary_path)
        .map_err(|error| HostFileError::io(temporary_path, error))?;
    file.write_all(content.as_bytes())
        .map_err(|error| HostFileError::io(temporary_path, error))
}

/// umask가 좁혔을 수 있는 권한을 지정한 값으로 맞춘다
fn write_new_file(
    temporary_path: &str,
    content: &str,
    mode: Option<u32>,
) -> Result<(), HostFileError> {
    create_exclusive_file(temporary_path, content, mode)?;
    if let Some(mode) = mode {
        std::fs::set_permissions(temporary_path, std::fs::Permissions::from_mode(mode))
            .map_err(|error| HostFileError::io(temporary_path, error))?;
    }
    Ok(())
}

/// 잠금을 잡은 동안 지금 내용을 읽고, 잠금이 여전히 내 것일 때만 임시 파일을 대상 파일로 교체한다
fn replace_locked_local_file(
    target_path: &str,
    temporary_path: &str,
    owner_path: &Path,
    lock_path: &Path,
    mode: Option<u32>,
    update: &mut TextFileUpdate<'_>,
) -> Result<(), HostFileError> {
    let current_content = match read_local_text(target_path) {
        Ok(content) => content,
        Err(error) if is_not_found(&error) => String::new(),
        Err(error) => return Err(HostFileError::io(target_path, error)),
    };
    let next_content = update(&current_content)?;
    let owner_metadata = std::fs::symlink_metadata(owner_path)
        .map_err(|error| HostFileError::io(&owner_path.to_string_lossy(), error))?;
    if !owner_metadata.is_dir() {
        return Err(HostFileError::Message(format!(
            "파일 잠금 소유권이 변경되었습니다: {}",
            lock_path.to_string_lossy()
        )));
    }
    write_new_file(temporary_path, &next_content, mode)?;
    std::fs::rename(temporary_path, target_path)
        .map_err(|error| HostFileError::io(target_path, error))
}

pub(super) fn update_local_text_file(
    target_path: &str,
    mode: Option<u32>,
    update: &mut TextFileUpdate<'_>,
) -> Result<(), HostFileError> {
    let lock_path = PathBuf::from(format!("{target_path}.lock"));
    let temporary_path = format!("{target_path}.{}.tmp", uuid::Uuid::new_v4());
    if let Some(directory) = Path::new(target_path)
        .parent()
        .filter(|directory| !directory.as_os_str().is_empty())
    {
        std::fs::create_dir_all(directory)
            .map_err(|error| HostFileError::io(target_path, error))?;
    }
    let owner_name = format!(
        "owner-{}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4(),
        local_hostname_hex()
    );
    let owner_path = lock_path.join(owner_name);
    acquire_local_file_lock(&lock_path, &owner_path)?;

    let result = replace_locked_local_file(
        target_path,
        &temporary_path,
        &owner_path,
        &lock_path,
        mode,
        update,
    );
    let temporary_cleanup = ignore_not_found(
        std::fs::remove_file(&temporary_path),
        Path::new(&temporary_path),
    );
    let lock_cleanup = remove_lock_owner(&owner_path, &lock_path);
    result.and(temporary_cleanup).and(lock_cleanup)
}

fn build_remote_umask_command(mode: Option<u32>) -> String {
    mode.map_or_else(String::new, |mode| format!("umask {:o} && ", !mode & 0o777))
}

/// 표준 입력의 base64를 임시 파일에 풀어 대상 파일로 교체하는 명령.
/// 내용을 명령줄에 싣지 않아 `ps`와 프로세스 감사 로그에 자격 증명이 남지 않는다.
/// 로그인 프로파일이 입력을 먼저 소비하면 빈 파일이 생기므로, 크기가 맞을 때만 교체해 기존 내용을 잃지 않는다.
fn build_remote_stdin_publish_command(
    temporary_path: &str,
    target_path: &str,
    expected_byte_length: &str,
    mode: Option<u32>,
) -> String {
    let quoted_temporary_path = quote_shell_argument(temporary_path);
    let chmod_command = mode.map_or_else(String::new, |mode| {
        format!("chmod {mode:o} {quoted_temporary_path} && ")
    });
    format!(
        "({}(base64 -d 2>/dev/null || base64 -D) > {quoted_temporary_path}) && [ \"$(wc -c < {quoted_temporary_path} | tr -d ' ')\" = \"{expected_byte_length}\" ] && {chmod_command}mv -f {quoted_temporary_path} {}",
        build_remote_umask_command(mode),
        quote_shell_argument(target_path)
    )
}

/// 잠금이 이미 있을 때 원격 셸이 사용 중·소유자 불명을 판정하는 부분
fn build_remote_lock_contention_lines(target_path: &str, lock_path: &str) -> Vec<String> {
    let busy = quote_shell_argument(REMOTE_FILE_UPDATE_BUSY);
    let unowned = quote_shell_argument(REMOTE_FILE_UPDATE_UNOWNED);
    let lock_error =
        quote_shell_argument(&format!("파일 잠금 경로를 확인하세요: {target_path}.lock"));
    vec![
        format!("if ! mkdir -m 700 {lock_path} 2>/dev/null; then"),
        format!("if ! test -e {lock_path} && ! test -L {lock_path}; then printf '%s\\n' {busy}; exit 0; fi;"),
        format!("test -d {lock_path} && ! test -L {lock_path} || {{ printf '%s\\n' {lock_error} >&2; exit 1; }};"),
        format!("set -- {lock_path}/owner-*;"),
        "if [ \"$#\" = 1 ] && test -d \"$1\" && ! test -L \"$1\"; then".into(),
        "__kanvibe_name=${1##*/}; if [ \"${__kanvibe_name##*-}\" != \"$__kanvibe_host\" ]; then".into(),
        format!("printf '%s\\n' {unowned}; exit 0; fi;"),
        "__kanvibe_pid=${__kanvibe_name#owner-}; __kanvibe_pid=${__kanvibe_pid%%-*};".into(),
        "case \"$__kanvibe_pid\" in \"\"|0|*[!0-9]*) exit 1;; esac;".into(),
        "if ! kill -0 \"$__kanvibe_pid\" 2>/dev/null && command -v ps >/dev/null 2>&1; then".into(),
        "__kanvibe_alive=$(ps -p \"$__kanvibe_pid\" -o pid= 2>/dev/null); __kanvibe_probe_status=$?;".into(),
        format!("if [ \"$__kanvibe_probe_status\" = 1 ] && [ -z \"$__kanvibe_alive\" ]; then rmdir \"$1\" 2>/dev/null && rmdir {lock_path}; fi; fi;"),
        format!("printf '%s\\n' {busy};"),
        format!("else printf '%s\\n' {unowned}; fi; exit 0; fi;"),
    ]
}

/// 잠금을 잡은 원격 셸이 지금 내용을 보내고, 표준 입력으로 받은 새 내용을 게시하는 부분
fn build_remote_publish_lines(
    target_path: &str,
    temporary_path: &str,
    mode: Option<u32>,
) -> Vec<String> {
    let quoted_target = quote_shell_argument(target_path);
    vec![
        format!("if test -e {quoted_target} || test -L {quoted_target}; then"),
        format!("test -f {quoted_target} || exit 1;"),
        format!(
            "__kanvibe_current=$(base64 -w 0 {quoted_target} 2>/dev/null || base64 < {quoted_target}) || exit $?;"
        ),
        "else __kanvibe_current=\"\"; fi;".into(),
        format!(
            "printf '%s\\t%s\\n' {} \"$(printf '%s' \"$__kanvibe_current\" | tr -d '\\n')\";",
            quote_shell_argument(REMOTE_FILE_UPDATE_READY)
        ),
        "IFS= read -r __kanvibe_length && IFS= read -r __kanvibe_next || exit 1;".into(),
        "case \"$__kanvibe_length\" in \"\"|*[!0-9]*) exit 1;; esac;".into(),
        "test -d \"$__kanvibe_owner\" && ! test -L \"$__kanvibe_owner\" || exit 1;".into(),
        format!(
            "printf '%s' \"$__kanvibe_next\" | {} || exit $?;",
            build_remote_stdin_publish_command(
                temporary_path,
                target_path,
                "$__kanvibe_length",
                mode
            )
        ),
        format!(
            "printf '%s\\n' {}",
            quote_shell_argument(REMOTE_FILE_UPDATE_DONE)
        ),
    ]
}

fn build_remote_file_update_command(
    target_path: &str,
    temporary_path: &str,
    mode: Option<u32>,
) -> String {
    let lock_path = quote_shell_argument(&format!("{target_path}.lock"));
    let owner_suffix = quote_shell_argument(&uuid::Uuid::new_v4().to_string());
    let cleanup = format!(
        "rm -f {}; rmdir \"$__kanvibe_owner\" && rmdir {lock_path}",
        quote_shell_argument(temporary_path)
    );
    let mut lines = vec![
        format!(
            "mkdir -p {} || exit $?;",
            quote_shell_argument(kanvibe_domain::project::posix_dirname(target_path))
        ),
        "__kanvibe_host=$(uname -n | tr -d \"\\n\" | od -An -tx1 | tr -d \" \\n\") || exit $?;"
            .into(),
        "test -n \"$__kanvibe_host\" || exit 1;".into(),
    ];
    lines.extend(build_remote_lock_contention_lines(target_path, &lock_path));
    lines.push(format!(
        "__kanvibe_owner={lock_path}/owner-$$-{owner_suffix}-\"$__kanvibe_host\";"
    ));
    lines.push(format!(
        "mkdir -m 700 \"$__kanvibe_owner\" || {{ rmdir {lock_path}; exit 1; }};"
    ));
    lines.push(format!(
        "trap {} 0; trap 'exit 1' HUP INT TERM;",
        quote_shell_argument(&cleanup)
    ));
    lines.extend(build_remote_publish_lines(
        target_path,
        temporary_path,
        mode,
    ));
    lines.join(" ")
}

/// 원격이 보낸 지금 내용(base64)에 callback을 적용해 `<바이트 수>\n<base64>\n` 응답을 만든다
fn answer_update_request(
    encoded_content: &str,
    update: &mut TextFileUpdate<'_>,
) -> Result<String, HostFileError> {
    let Some(content) = decode_canonical_base64(encoded_content) else {
        return Err(HostFileError::Message(
            "원격 파일 갱신 요청이 손상되었습니다.".into(),
        ));
    };
    let next_content = update(&String::from_utf8_lossy(&content))?;
    Ok(format!(
        "{}\n{}\n",
        next_content.len(),
        STANDARD.encode(next_content.as_bytes())
    ))
}

enum RemoteUpdateMessage {
    Request(String),
    Finished(Result<String, ExecGitError>),
}

struct RemoteUpdateExchange {
    output: String,
    has_response_started: bool,
}

/// 원격 셸의 갱신 요청을 갱신 callback이 있는 스레드로 넘기고, 그 응답을 기다려 원격 stdin으로 돌려주는 응답기
fn build_update_responder(
    request_sender: mpsc::Sender<RemoteUpdateMessage>,
    reply_receiver: mpsc::Receiver<Result<String, ExecGitError>>,
) -> RemoteStdinResponder {
    let reply_receiver = Mutex::new(reply_receiver);
    let unavailable = || ExecGitError::new("원격 파일 갱신 응답을 만들 수 없습니다.");
    RemoteStdinResponder {
        marker: REMOTE_FILE_UPDATE_READY.into(),
        respond: Arc::new(move |payload: &str| {
            let message = RemoteUpdateMessage::Request(payload.to_string());
            request_sender.send(message).map_err(|_| unavailable())?;
            let reply = reply_receiver.lock().unwrap().recv();
            reply.unwrap_or_else(|_| Err(unavailable()))
        }),
    }
}

/// 명령이 끝날 때까지 갱신 요청에 답한다. callback 오류가 있었으면 전송 오류보다 그 오류를 먼저 돌려준다
fn serve_update_requests(
    message_receiver: &mpsc::Receiver<RemoteUpdateMessage>,
    reply_sender: &mpsc::Sender<Result<String, ExecGitError>>,
    update: &mut TextFileUpdate<'_>,
) -> Result<RemoteUpdateExchange, HostFileError> {
    let mut has_response_started = false;
    let mut update_error = None;
    loop {
        let message = message_receiver
            .recv()
            .expect("명령 스레드는 끝날 때 결과를 보낸다");
        let encoded_content = match message {
            RemoteUpdateMessage::Request(encoded_content) => encoded_content,
            RemoteUpdateMessage::Finished(Ok(output)) => {
                return Ok(RemoteUpdateExchange {
                    output,
                    has_response_started,
                });
            }
            RemoteUpdateMessage::Finished(Err(error)) => {
                return Err(update_error.unwrap_or(HostFileError::Exec(error)));
            }
        };
        has_response_started = true;
        let reply = answer_update_request(&encoded_content, update);
        let response = reply
            .as_ref()
            .map(String::clone)
            .map_err(|error| ExecGitError::new(error.to_string()));
        let _ = reply_sender.send(response);
        update_error = reply.err().or(update_error);
    }
}

/// stdin 응답기는 `'static` callback이어야 하므로, 명령은 범위 스레드에서 돌리고
/// 빌려 온 갱신 callback은 이 스레드에서 실행해 그 결과만 응답기로 돌려보낸다
fn run_remote_update_exchange(
    runner: &dyn ShellCommandRunner,
    command: &str,
    host: &str,
    options: &ExecGitOptions,
    update: &mut TextFileUpdate<'_>,
) -> Result<RemoteUpdateExchange, HostFileError> {
    let (message_sender, message_receiver) = mpsc::channel::<RemoteUpdateMessage>();
    let (reply_sender, reply_receiver) = mpsc::channel::<Result<String, ExecGitError>>();
    let responder = build_update_responder(message_sender.clone(), reply_receiver);
    let exec_options = ExecGitOptions {
        max_buffer_bytes: Some(REMOTE_TEXT_FILE_MAX_BUFFER_BYTES),
        remote_stdin_responder: Some(responder),
        ..options.clone()
    };
    std::thread::scope(|scope| {
        scope.spawn(move || {
            let result = runner.exec_git(command, Some(host), &exec_options);
            let _ = message_sender.send(RemoteUpdateMessage::Finished(result));
        });
        serve_update_requests(&message_receiver, &reply_sender, update)
    })
}

/// 원격 저장소의 잠금을 한 SSH 왕복 안에서 잡고, 사용 중이면 잠시 뒤 다시 시도한다
pub(super) fn update_remote_text_file(
    runner: &dyn ShellCommandRunner,
    target_path: &str,
    host: &str,
    mode: Option<u32>,
    options: &ExecGitOptions,
    update: &mut TextFileUpdate<'_>,
) -> Result<(), HostFileError> {
    let temporary_path = format!("{target_path}.{}.tmp", uuid::Uuid::new_v4());
    let command = build_remote_file_update_command(target_path, &temporary_path, mode);
    let unknown_result = || {
        HostFileError::Message(format!(
            "원격 파일 갱신 결과를 확인할 수 없습니다: {target_path}"
        ))
    };
    let mut unowned_observations = 0;
    for _ in 0..FILE_UPDATE_LOCK_ATTEMPTS {
        let exchange = run_remote_update_exchange(runner, &command, host, options, update)?;
        let records: Vec<&str> = exchange.output.split('\n').collect();
        let is_published = records.contains(&REMOTE_FILE_UPDATE_DONE);
        if exchange.has_response_started && is_published {
            return Ok(());
        }
        if exchange.has_response_started || is_published {
            return Err(unknown_result());
        }
        if records.contains(&REMOTE_FILE_UPDATE_UNOWNED) {
            unowned_observations += 1;
            if unowned_observations >= FILE_UPDATE_OWNER_INITIALIZATION_ATTEMPTS {
                return Err(unowned_lock(&format!("{target_path}.lock")));
            }
        } else if records.contains(&REMOTE_FILE_UPDATE_BUSY) {
            unowned_observations = 0;
        } else {
            return Err(unknown_result());
        }
        std::thread::sleep(FILE_UPDATE_LOCK_RETRY);
    }
    Err(lock_wait_timeout(&format!("{target_path}.lock")))
}
