use std::os::unix::fs::PermissionsExt;
use std::sync::Mutex;

use super::*;

fn ssh_config() -> SshHostConfig {
    SshHostConfig {
        host: "app-prod".into(),
        hostname: "example.com".into(),
        port: 2202,
        username: "tester".into(),
        private_key_path: "/tmp/test-key".into(),
    }
}

/// scp 인자, 전송 시점의 로컬 파일 내용, 그 파일 권한
type RecordedCopy = (Vec<String>, Vec<u8>, u32);

/// 받은 인자와, 전송 시점에 로컬 파일 내용·권한을 기록한다
struct RecordingScpTransport {
    result: Result<(), String>,
    copies: Mutex<Vec<RecordedCopy>>,
}

impl RecordingScpTransport {
    fn new(result: Result<(), String>) -> Arc<Self> {
        Arc::new(Self {
            result,
            copies: Mutex::new(Vec::new()),
        })
    }

    fn copies(&self) -> Vec<RecordedCopy> {
        self.copies.lock().unwrap().clone()
    }
}

impl ScpTransport for RecordingScpTransport {
    fn copy(&self, scp_arguments: &[String]) -> Result<(), String> {
        let local_path = &scp_arguments[scp_arguments.len() - 2];
        let content = fs::read(local_path).unwrap();
        let mode = fs::metadata(local_path).unwrap().permissions().mode() & 0o777;
        self.copies
            .lock()
            .unwrap()
            .push((scp_arguments.to_vec(), content, mode));
        self.result.clone()
    }
}

fn paste_with(transport: Arc<RecordingScpTransport>, root: &Path) -> RemoteImagePaste {
    RemoteImagePaste::with_uuid_source(transport, root.to_path_buf(), || "fixed-uuid".into())
}

fn leftover_entries(root: &Path) -> usize {
    fs::read_dir(root).unwrap().count()
}

#[test]
fn decodes_the_base64_payload_of_a_data_url() {
    let bytes = decode_data_url_to_bytes("data:image/png;base64,aGVsbG8=").unwrap();

    assert_eq!(bytes, b"hello");
}

#[test]
fn rejects_a_data_url_without_a_comma() {
    let error = decode_data_url_to_bytes("not-a-data-url").unwrap_err();

    assert_eq!(error.message, "잘못된 이미지 데이터입니다.");
}

#[test]
fn builds_a_fixed_tmp_path_from_the_uuid() {
    assert_eq!(
        build_remote_image_paste_path("fixed-uuid"),
        "/tmp/kanvibe-paste-fixed-uuid.png"
    );
}

#[test]
fn keeps_a_generic_attachment_extension_on_the_same_transport() {
    let root = tempfile::tempdir().unwrap();
    let transport = RecordingScpTransport::new(Ok(()));
    let paste = paste_with(Arc::clone(&transport), root.path());

    let remote_path = paste
        .transfer_file_to_remote_host(&ssh_config(), b"document", "pdf")
        .unwrap();
    let rejected = paste.transfer_file_to_remote_host(&ssh_config(), b"bad", "../bad");

    assert_eq!(remote_path, "/tmp/kanvibe-paste-fixed-uuid.pdf");
    let (arguments, content, mode) = transport.copies().remove(0);
    assert!(arguments[8].ends_with("/fixed-uuid.pdf"));
    assert_eq!(arguments[9], "app-prod:/tmp/kanvibe-paste-fixed-uuid.pdf");
    assert_eq!((content.as_slice(), mode), (b"document".as_slice(), 0o600));
    assert_eq!(
        rejected.unwrap_err().message,
        "첨부 파일 확장자가 올바르지 않습니다."
    );
    assert_eq!(transport.copies().len(), 1);
}

#[test]
fn uploads_the_image_with_scp_and_returns_the_remote_path() {
    let root = tempfile::tempdir().unwrap();
    let transport = RecordingScpTransport::new(Ok(()));
    let paste = paste_with(Arc::clone(&transport), root.path());

    let remote_path = paste
        .transfer_image_to_remote_host(&ssh_config(), b"image-bytes")
        .unwrap();

    assert_eq!(remote_path, "/tmp/kanvibe-paste-fixed-uuid.png");
    let (arguments, content, _) = transport.copies().remove(0);
    let local_path = arguments[8].clone();
    let local_directory = Path::new(&local_path).parent().unwrap();
    assert!(
        local_directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("kanvibe-paste-")
    );
    assert_eq!(
        arguments,
        [
            "-i",
            "/tmp/test-key",
            "-P",
            "2202",
            "-o",
            "BatchMode=yes",
            "-o",
            "IdentitiesOnly=yes",
            local_path.as_str(),
            "app-prod:/tmp/kanvibe-paste-fixed-uuid.png"
        ]
    );
    assert_eq!(content, b"image-bytes");
    assert_eq!(leftover_entries(root.path()), 0);
}

#[test]
fn reports_scp_stderr_and_still_removes_the_local_directory() {
    let root = tempfile::tempdir().unwrap();
    let bin_directory = root.path().join("bin");
    fs::create_dir(&bin_directory).unwrap();
    let fake_scp = bin_directory.join("scp");
    fs::write(
        &fake_scp,
        "#!/bin/sh\necho 'Permission denied' >&2\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&fake_scp, fs::Permissions::from_mode(0o755)).unwrap();
    let temp_root = root.path().join("tmp");
    fs::create_dir(&temp_root).unwrap();
    let transport = SystemScpTransport::new(ShellEnvironment::from([(
        "PATH".to_string(),
        format!("{}:/usr/bin:/bin", bin_directory.display()),
    )]));
    let paste = RemoteImagePaste::new(Arc::new(transport), temp_root.clone());

    let error = paste
        .transfer_image_to_remote_host(&ssh_config(), b"image-bytes")
        .unwrap_err();

    assert_eq!(error.message, "scp 전송 실패 (exit 1): Permission denied");
    assert_eq!(leftover_entries(&temp_root), 0);
}

#[test]
fn reports_a_failed_scp_spawn_and_still_cleans_up() {
    let root = tempfile::tempdir().unwrap();
    let transport = SystemScpTransport::new(ShellEnvironment::from([(
        "PATH".to_string(),
        root.path().join("no-bin").to_string_lossy().into_owned(),
    )]));
    let paste = RemoteImagePaste::with_uuid_source(Arc::new(transport), root.path().into(), || {
        "fixed-uuid".into()
    });

    let error = paste
        .transfer_image_to_remote_host(&ssh_config(), b"image-bytes")
        .unwrap_err();

    assert!(error.message.contains("No such file or directory"));
    assert_eq!(leftover_entries(root.path()), 0);
}

#[test]
fn the_local_temp_directory_is_owner_only_and_retries_a_taken_name() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("kanvibe-paste-aaaaaa")).unwrap();
    let mut suffixes = ["aaaaaa", "bbbbbb"].into_iter();

    let directory =
        create_private_directory(root.path(), || suffixes.next().unwrap().to_string()).unwrap();

    assert_eq!(directory, root.path().join("kanvibe-paste-bbbbbb"));
    let mode = fs::metadata(&directory).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
}
