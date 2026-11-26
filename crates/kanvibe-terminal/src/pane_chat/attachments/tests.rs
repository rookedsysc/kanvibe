use std::fs;
use std::sync::{Arc, Mutex};
use std::thread;

use kanvibe_process::ssh_config::{SshConfigReader, SshHostConfig};

use super::{
    PaneChatAttachmentInput, PaneChatAttachmentKind, PaneChatAttachmentOwner,
    PaneChatAttachmentStore, RemoteAttachmentUploader, ResolvedPaneChatAttachment,
};
use crate::test_support::ScriptedShellRunner;

/// 올린 파일과 설정을 기록하고 정해 둔 원격 경로를 돌려주는 scp 대역
struct RecordingUploader {
    remote_path: String,
    transfers: Mutex<Vec<(SshHostConfig, Vec<u8>, String)>>,
}

impl RemoteAttachmentUploader for RecordingUploader {
    fn transfer_file_to_remote_host(
        &self,
        config: &SshHostConfig,
        content: &[u8],
        extension: &str,
    ) -> Result<String, String> {
        let transfer = (config.clone(), content.to_vec(), extension.to_string());
        self.transfers.lock().unwrap().push(transfer);
        Ok(self.remote_path.clone())
    }
}

struct AttachmentFixture {
    _home_directory: tempfile::TempDir,
    data_directory: tempfile::TempDir,
    runner: Arc<ScriptedShellRunner>,
    uploader: Arc<RecordingUploader>,
    store: PaneChatAttachmentStore,
}

impl AttachmentFixture {
    /// `ssh_config`를 `~/.ssh/config`로 둔 임시 HOME에서 저장소를 만든다
    fn new(ssh_config: &str) -> Self {
        let home_directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(home_directory.path().join(".ssh")).unwrap();
        fs::write(home_directory.path().join(".ssh/config"), ssh_config).unwrap();
        let data_directory = tempfile::tempdir().unwrap();
        let runner = ScriptedShellRunner::new();
        let uploader = Arc::new(RecordingUploader {
            remote_path: "/tmp/kanvibe-paste-fixture.pdf".to_string(),
            transfers: Mutex::new(Vec::new()),
        });
        let reader = SshConfigReader::new(home_directory.path().to_str().unwrap());
        let store = PaneChatAttachmentStore::new(
            data_directory.path().to_path_buf(),
            runner.clone(),
            Arc::new(reader),
            uploader.clone(),
        );
        Self {
            _home_directory: home_directory,
            data_directory,
            runner,
            uploader,
            store,
        }
    }
}

fn owner() -> PaneChatAttachmentOwner {
    PaneChatAttachmentOwner {
        task_id: "attachment-task".to_string(),
        target_token: "attachment-token".to_string(),
        identity: "attachment-cli".to_string(),
        ssh_host: None,
    }
}

fn input(name: &str, mime_type: &str, data_url: &str) -> PaneChatAttachmentInput {
    PaneChatAttachmentInput {
        name: name.to_string(),
        mime_type: mime_type.to_string(),
        data_url: data_url.to_string(),
    }
}

fn hello_text(name: &str) -> PaneChatAttachmentInput {
    input(name, "text/plain", "data:text/plain;base64,aGVsbG8=")
}

fn pdf_document() -> PaneChatAttachmentInput {
    input(
        "document.pdf",
        "application/pdf",
        "data:application/pdf;base64,JVBERi0xLjc=",
    )
}

#[test]
fn stages_bytes_in_owned_path_and_removes_unsent_file() {
    let fixture = AttachmentFixture::new("");
    let owner = owner();

    let file = fixture
        .store
        .store_pane_chat_attachment(&owner, &hello_text("설계 문서.txt"))
        .unwrap();

    assert_eq!(file.name, "설계 문서.txt");
    assert_eq!(file.kind, PaneChatAttachmentKind::File);
    assert_eq!(file.mime_type, "text/plain");
    assert_eq!(file.size_bytes, 5);
    let stored = fixture
        .store
        .resolve_pane_chat_attachments(&owner, std::slice::from_ref(&file.id))
        .unwrap();
    assert!(
        stored[0]
            .path
            .starts_with(fixture.data_directory.path().to_str().unwrap())
    );
    assert_eq!(fs::read_to_string(&stored[0].path).unwrap(), "hello");
    fixture
        .store
        .discard_pane_chat_attachment(&owner.task_id, &owner.target_token, &file.id)
        .unwrap();
    assert!(fs::metadata(&stored[0].path).is_err());
}

#[test]
fn does_not_substitute_attachments_from_another_pane_host_or_cli() {
    let fixture = AttachmentFixture::new("");
    let owner = owner();
    let file = fixture
        .store
        .store_pane_chat_attachment(&owner, &hello_text("readme.txt"))
        .unwrap();
    let ids = [file.id.clone()];

    let other_pane = PaneChatAttachmentOwner {
        target_token: "other-token".to_string(),
        ..owner.clone()
    };
    let other_host = PaneChatAttachmentOwner {
        ssh_host: Some("other-host".to_string()),
        ..owner.clone()
    };
    assert!(
        fixture
            .store
            .resolve_pane_chat_attachments(&other_pane, &ids)
            .is_err()
    );
    assert!(
        fixture
            .store
            .resolve_pane_chat_attachments(&other_host, &ids)
            .is_err()
    );
    assert!(
        fixture
            .store
            .resolve_pane_chat_attachments(&owner, &["../../arbitrary".to_string()])
            .is_err()
    );
    assert!(
        fixture
            .store
            .discard_pane_chat_attachment("other-task", &owner.target_token, &file.id)
            .is_err()
    );
}

#[test]
fn keeps_submitted_file_readable_after_composer_chip_is_removed() {
    let fixture = AttachmentFixture::new("");
    let owner = owner();
    let file = fixture
        .store
        .store_pane_chat_attachment(&owner, &hello_text("readme.txt"))
        .unwrap();
    let ids = [file.id.clone()];
    let stored = fixture
        .store
        .resolve_pane_chat_attachments(&owner, &ids)
        .unwrap();

    fixture
        .store
        .retain_pane_chat_attachments(&owner, &ids)
        .unwrap();
    fixture
        .store
        .discard_pane_chat_attachment(&owner.task_id, &owner.target_token, &file.id)
        .unwrap();

    assert_eq!(fs::read_to_string(&stored[0].path).unwrap(), "hello");
}

#[test]
fn rejects_forged_image_bytes_before_creating_attachment() {
    let fixture = AttachmentFixture::new("");

    let forged = input("fake.png", "image/png", "data:image/png;base64,aGVsbG8=");
    let error = fixture
        .store
        .store_pane_chat_attachment(&owner(), &forged)
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "지원하지 않거나 손상된 이미지입니다. PNG, JPEG, GIF, WebP를 사용해 주세요."
    );
    assert!(
        !fixture
            .data_directory
            .path()
            .join("pane-chat-attachments")
            .exists()
    );
}

#[test]
fn bounds_concurrent_uploads_at_ten_per_cli() {
    let fixture = AttachmentFixture::new("");
    let owner = owner();

    let attempts: Vec<bool> = thread::scope(|scope| {
        let uploads: Vec<_> = (0..11)
            .map(|_| {
                scope.spawn(|| {
                    fixture
                        .store
                        .store_pane_chat_attachment(&owner, &hello_text("readme.txt"))
                        .is_ok()
                })
            })
            .collect();
        uploads
            .into_iter()
            .map(|upload| upload.join().unwrap())
            .collect()
    });

    assert_eq!(attempts.iter().filter(|is_stored| **is_stored).count(), 10);
    assert_eq!(attempts.iter().filter(|is_stored| !**is_stored).count(), 1);
}

/// `~/.ssh/config`의 `own-host` 항목을 해석한 설정
fn own_host_config() -> SshHostConfig {
    SshHostConfig {
        host: "own-host".to_string(),
        hostname: "127.0.0.1".to_string(),
        port: 2222,
        username: "fixture".to_string(),
        private_key_path: "/fixture/key".to_string(),
    }
}

/// SSH 설정에 없는 호스트면 업로드를 시도하지 않고 SSH 호스트 오류로 거절한다
fn assert_unknown_host_is_not_uploaded(remote_owner: &PaneChatAttachmentOwner) {
    let unknown_host = AttachmentFixture::new("");
    let error = unknown_host
        .store
        .store_pane_chat_attachment(remote_owner, &pdf_document())
        .unwrap_err();
    assert!(error.to_string().contains("SSH"));
    assert!(unknown_host.uploader.transfers.lock().unwrap().is_empty());
}

#[test]
fn stages_and_removes_remote_file_only_on_its_verified_host() {
    let ssh_config = "Host own-host\n  HostName 127.0.0.1\n  User fixture\n  Port 2222\n  IdentityFile /fixture/key\n";
    let fixture = AttachmentFixture::new(ssh_config);
    let remote_owner = PaneChatAttachmentOwner {
        ssh_host: Some("own-host".to_string()),
        ..owner()
    };

    let store = &fixture.store;
    let file = store
        .store_pane_chat_attachment(&remote_owner, &pdf_document())
        .unwrap();

    let transfers = fixture.uploader.transfers.lock().unwrap().clone();
    assert_eq!(
        transfers,
        vec![(own_host_config(), b"%PDF-1.7".to_vec(), "pdf".to_string())]
    );
    let ids = [file.id.clone()];
    let expected = ResolvedPaneChatAttachment {
        id: file.id.clone(),
        path: "/tmp/kanvibe-paste-fixture.pdf".to_string(),
        kind: PaneChatAttachmentKind::File,
    };
    assert_eq!(
        store
            .resolve_pane_chat_attachments(&remote_owner, &ids)
            .unwrap(),
        vec![expected]
    );
    assert!(store.resolve_pane_chat_attachments(&owner(), &ids).is_err());
    store
        .discard_pane_chat_attachment(&remote_owner.task_id, &remote_owner.target_token, &file.id)
        .unwrap();
    let calls = fixture.runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].command,
        "rm -f -- '/tmp/kanvibe-paste-fixture.pdf'"
    );
    assert_eq!(calls[0].ssh_host.as_deref(), Some("own-host"));

    assert_unknown_host_is_not_uploaded(&remote_owner);
}

#[test]
fn fingerprints_attachments_by_name_type_size_and_content() {
    let fixture = AttachmentFixture::new("");
    let owner = owner();
    let file = fixture
        .store
        .store_pane_chat_attachment(&owner, &hello_text("readme.txt"))
        .unwrap();

    let fingerprint = fixture
        .store
        .fingerprint_pane_chat_attachments(&owner, &[file.id])
        .unwrap();

    assert_eq!(
        fingerprint,
        r#"[["readme.txt","text/plain",5,"2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"]]"#
    );
}
