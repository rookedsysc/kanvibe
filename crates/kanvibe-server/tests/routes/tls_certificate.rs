//! TS `mobileTlsCertificate.test.mjs`. 여기만 실제 RSA 인증서를 만든다.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use kanvibe_server::server::{ServerConfig, ServerState, start_server};
use kanvibe_server::tls::{
    PRIVATE_KEY_FILE_NAME, certificate_fingerprint, load_or_create_identity,
};

use crate::harness::*;

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path)
        .expect("파일 정보")
        .permissions()
        .mode()
        & 0o777
}

// parity: TS-electron-mobileTlsCertificate-f1456a
#[test]
fn private_key_is_readable_only_by_the_owner() {
    let parent = tempfile::tempdir().expect("임시 폴더");
    let directory = parent.path().join("mobile-tls");
    load_or_create_identity(&directory).expect("인증서 생성");
    assert_eq!(mode(&directory.join(PRIVATE_KEY_FILE_NAME)), 0o600);
    assert_eq!(mode(&directory), 0o700);
}

// parity: TS-electron-mobileTlsCertificate-dcc0ea
#[test]
fn loading_again_returns_the_same_certificate_so_pinned_fingerprints_survive() {
    let parent = tempfile::tempdir().expect("임시 폴더");
    let directory = parent.path().join("mobile-tls");
    let first = load_or_create_identity(&directory).expect("인증서 생성");
    let second = load_or_create_identity(&directory).expect("인증서 읽기");
    assert_eq!(second, first);
}

// parity: TS-electron-mobileTlsCertificate-ae4bfa
#[tokio::test]
async fn generated_certificate_serves_tls_requests() {
    let parent = tempfile::tempdir().expect("임시 폴더");
    let tls_directory = parent.path().join("mobile-tls");
    let fake = FakeBridge::new();
    let state = ServerState::new(fake.clone(), fake.clone(), Default::default());
    let loopback = "127.0.0.1:0".parse().expect("주소");
    let config = ServerConfig {
        plaintext_address: loopback,
        tls_address: loopback,
        tls_directory: tls_directory.clone(),
    };
    let running = start_server(&config, state).await.expect("서버 시작");
    let fingerprint =
        certificate_fingerprint(&load_or_create_identity(&tls_directory).expect("만든 인증서"))
            .expect("지문");
    let stream = crate::harness::tls_client::connect_tls(
        running.tls_address.expect("TLS 주소"),
        &fingerprint,
    )
    .await;
    let call = HttpCall::get("/api/mobile/board").auth(GOOD);
    let reply = crate::harness::http_client::exchange(stream, &call)
        .await
        .reply();
    assert_eq!(reply.status, 200);
}
