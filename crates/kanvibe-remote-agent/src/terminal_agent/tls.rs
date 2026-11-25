//! 직접 연결 WSS의 TLS 설정. 데스크탑이 stdin으로 준 자체 서명 인증서를 ring 공급자로만 쓴다(OpenSSL 없음).

use std::sync::Arc;

use rustls::ServerConfig;
use rustls::crypto::ring::default_provider;
use sha2::{Digest, Sha256};

/// 서버 설정과 인증서 지문(DER의 sha256, 소문자 hex). 모바일은 이 지문으로 인증서를 고정한다
pub(super) fn server_config(
    certificate_pem: &str,
    private_key_pem: &str,
) -> Result<(Arc<ServerConfig>, String), String> {
    let invalid = |_| "Invalid terminal certificate".to_string();
    let certificates = rustls_pemfile::certs(&mut certificate_pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .map_err(invalid)?;
    let leaf = certificates.first().ok_or("Invalid terminal certificate")?;
    let fingerprint = hex::encode(Sha256::digest(leaf.as_ref()));
    let private_key = rustls_pemfile::private_key(&mut private_key_pem.as_bytes())
        .map_err(invalid)?
        .ok_or("Invalid terminal certificate key")?;
    let config = ServerConfig::builder_with_provider(Arc::new(default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|error| error.to_string())?
        .with_no_client_auth()
        .with_single_cert(certificates, private_key)
        .map_err(|error| error.to_string())?;
    Ok((Arc::new(config), fingerprint))
}
