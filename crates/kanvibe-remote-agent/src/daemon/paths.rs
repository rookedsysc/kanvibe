//! daemon의 비공개 디렉터리, 시작 잠금, 남은 소켓 정리(TS `preparePaths`, `acquireStartLock`, `removeStaleSocket`).
//!
//! `/tmp/kv-<uid>`와 그 아래 namespace 디렉터리는 본인 소유이고 심볼릭 링크가 아닌 0700 디렉터리여야 한다.
//! 같은 namespace에 daemon은 하나만 뜨도록 `start.lock/pid`를 쥐고, 살아 있는 소켓은 지우지 않는다.

use std::fs;
use std::io::{ErrorKind, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::protocol::{
    HostSocketPaths, current_uid, host_socket_paths_in, is_valid_host_namespace,
};

const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
const SOCKET_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// 본인 소유의 0700 실제 디렉터리인지 확인한다
pub fn assert_private_directory(directory: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(directory).map_err(|error| error.to_string())?;
    let is_private = metadata.is_dir()
        && !metadata.file_type().is_symlink()
        && metadata.uid() == current_uid()
        && metadata.mode() & 0o777 == PRIVATE_DIRECTORY_MODE;
    if !is_private {
        return Err("Untrusted host private directory".into());
    }
    Ok(())
}

fn create_private_directory(directory: &Path) -> Result<(), String> {
    match fs::DirBuilder::new()
        .mode(PRIVATE_DIRECTORY_MODE)
        .create(directory)
    {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

/// namespace 디렉터리를 만들고 확인한다. `root`가 없으면 `/tmp/kv-<uid>`를 쓴다
pub fn prepare_paths(namespace: &str, root: Option<&Path>) -> Result<HostSocketPaths, String> {
    if !is_valid_host_namespace(namespace) {
        return Err("Unsupported host runtime or configuration".into());
    }
    let default_parent = PathBuf::from(format!("/tmp/kv-{}", current_uid()));
    let parent = root.unwrap_or(&default_parent);
    create_private_directory(parent)?;
    assert_private_directory(parent)?;
    let paths = host_socket_paths_in(&parent.to_string_lossy(), namespace);
    let directory = Path::new(&paths.directory);
    create_private_directory(directory)?;
    assert_private_directory(directory)?;
    Ok(paths)
}

/// namespace 디렉터리의 `start.lock`. 쥔 daemon이 끝날 때 `release`로 지운다
pub struct StartLock {
    lock_path: PathBuf,
    inode: u64,
}

impl StartLock {
    pub fn acquire(directory: &Path) -> Result<Self, String> {
        let lock_path = directory.join("start.lock");
        loop {
            match fs::DirBuilder::new()
                .mode(PRIVATE_DIRECTORY_MODE)
                .create(&lock_path)
            {
                Ok(()) => return Self::write_owner(lock_path),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                    remove_abandoned_lock(&lock_path)?;
                }
                Err(error) => return Err(error.to_string()),
            }
        }
    }

    fn write_owner(lock_path: PathBuf) -> Result<Self, String> {
        let mut pid_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(lock_path.join("pid"))
            .map_err(|error| error.to_string())?;
        pid_file
            .write_all(std::process::id().to_string().as_bytes())
            .map_err(|error| error.to_string())?;
        let inode = fs::symlink_metadata(&lock_path)
            .map_err(|error| error.to_string())?
            .ino();
        Ok(Self { lock_path, inode })
    }

    /// 내가 만든 잠금일 때만 지운다
    pub fn release(&self) {
        let is_owned = fs::symlink_metadata(&self.lock_path)
            .is_ok_and(|metadata| metadata.ino() == self.inode);
        if is_owned {
            let _ = fs::remove_dir_all(&self.lock_path);
        }
    }
}

/// 잠금을 쥔 프로세스가 사라졌을 때만 잠금을 지운다. 살아 있으면 다른 daemon이 뜨는 중이거나 떠 있다
fn remove_abandoned_lock(lock_path: &Path) -> Result<(), String> {
    assert_private_directory(lock_path)?;
    let before = fs::symlink_metadata(lock_path).map_err(|error| error.to_string())?;
    let owner_path = lock_path.join("pid");
    let owner = fs::symlink_metadata(&owner_path).map_err(|error| error.to_string())?;
    if !owner.is_file() || owner.file_type().is_symlink() || owner.uid() != current_uid() {
        return Err("Untrusted host start lock".into());
    }
    let pid = fs::read_to_string(&owner_path)
        .ok()
        .and_then(|text| text.trim().parse::<i32>().ok())
        .filter(|pid| *pid >= 1)
        .ok_or("Invalid host start lock")?;
    if is_process_alive(pid) {
        return Err("Host daemon is already starting or running".into());
    }
    let after = fs::symlink_metadata(lock_path).map_err(|error| error.to_string())?;
    if after.ino() != before.ino() {
        return Err("Host start lock changed".into());
    }
    fs::remove_dir_all(lock_path).map_err(|error| error.to_string())
}

/// `kill(pid, 0)`이 ESRCH가 아니면 살아 있다고 본다
pub fn is_process_alive(pid: i32) -> bool {
    // SAFETY: 신호 0은 존재 확인만 하고 프로세스에 아무것도 보내지 않는다
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// 아무도 듣지 않는 옛 소켓만 지운다. 살아 있는 소켓이면 다른 daemon이 떠 있다는 오류다
pub fn remove_stale_socket(socket_path: &Path) -> Result<(), String> {
    let before = match fs::symlink_metadata(socket_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    use std::os::unix::fs::FileTypeExt;
    if !before.file_type().is_socket() || before.uid() != current_uid() {
        return Err("Untrusted host socket".into());
    }
    match connect_with_timeout(socket_path) {
        Ok(_) => return Err("Host daemon is already running".into()),
        Err(error) if error.kind() == ErrorKind::ConnectionRefused => {}
        Err(error) => return Err(error.to_string()),
    }
    let after = fs::symlink_metadata(socket_path).map_err(|error| error.to_string())?;
    if after.ino() != before.ino() {
        return Err("Host socket changed during probe".into());
    }
    fs::remove_file(socket_path).map_err(|error| error.to_string())
}

/// Unix 소켓 연결. 받아 주지 않는 소켓에서 오래 막히지 않도록 쓰기·읽기 제한을 건다
pub fn connect_with_timeout(socket_path: &Path) -> std::io::Result<UnixStream> {
    let stream = UnixStream::connect(socket_path)?;
    stream.set_write_timeout(Some(SOCKET_PROBE_TIMEOUT))?;
    Ok(stream)
}
