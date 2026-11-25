//! 모바일 TLS 포트의 자체 서명 인증서(TS `electron/mobileTlsCertificate.js`).
//!
//! 앱은 인증서 체인이 아니라 DER의 sha256 지문으로 서버를 믿는다. 지문이 바뀌면 모든 기기를 다시 페어링해야 하므로
//! 두 파일이 다 있으면 절대 새로 만들지 않고 바이트 그대로 쓴다. 처음 만들 때는 TS `selfsigned`와 같은 모양
//! (CN `KanVibe Desktop`, RSA 2048, sha256 서명, 20년)으로 만들고, 키는 `selfsigned`처럼 PKCS#1 PEM으로 쓴다.

use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::pkcs8::{EncodePrivateKey, LineEnding};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use sha2::{Digest, Sha256};

pub const CERTIFICATE_FILE_NAME: &str = "certificate.pem";
pub const PRIVATE_KEY_FILE_NAME: &str = "private-key.pem";
const COMMON_NAME: &str = "KanVibe Desktop";
const VALIDITY_DAYS: i64 = 20 * 365;
const RSA_KEY_BITS: usize = 2048;

#[derive(Debug, thiserror::Error)]
pub enum TlsSetupError {
    #[error("인증서 파일을 다루지 못했습니다: {0}")]
    Io(#[from] io::Error),
    #[error("인증서를 만들지 못했습니다: {0}")]
    Generate(String),
    #[error("인증서를 읽지 못했습니다: {0}")]
    Load(String),
}

/// 인증서와 키 PEM 원문
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsIdentityPem {
    pub certificate: String,
    pub private_key: String,
}

pub fn load_or_create_tls_acceptor(
    directory: &Path,
) -> Result<tokio_rustls::TlsAcceptor, TlsSetupError> {
    let identity = load_or_create_identity(directory)?;
    let config = build_server_config(&identity)?;
    Ok(tokio_rustls::TlsAcceptor::from(Arc::new(config)))
}

/// 두 파일이 모두 있으면 그대로 읽고, 하나라도 없으면 새로 만든다
pub fn load_or_create_identity(directory: &Path) -> Result<TlsIdentityPem, TlsSetupError> {
    let (certificate_path, private_key_path) = identity_paths(directory);
    match (
        fs::read_to_string(&certificate_path),
        fs::read_to_string(&private_key_path),
    ) {
        (Ok(certificate), Ok(private_key)) => {
            return Ok(TlsIdentityPem {
                certificate,
                private_key,
            });
        }
        (Err(error), _) | (_, Err(error)) if error.kind() != io::ErrorKind::NotFound => {
            return Err(error.into());
        }
        _ => {}
    }
    let identity = generate_identity()?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)?;
    write_with_mode(&private_key_path, &identity.private_key, 0o600)?;
    write_with_mode(&certificate_path, &identity.certificate, 0o644)?;
    Ok(identity)
}

fn identity_paths(directory: &Path) -> (PathBuf, PathBuf) {
    (
        directory.join(CERTIFICATE_FILE_NAME),
        directory.join(PRIVATE_KEY_FILE_NAME),
    )
}

fn write_with_mode(path: &Path, content: &str, mode: u32) -> io::Result<()> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(path)?;
    file.write_all(content.as_bytes())
}

fn generate_identity() -> Result<TlsIdentityPem, TlsSetupError> {
    let failure = |error: &dyn std::fmt::Display| TlsSetupError::Generate(error.to_string());
    let rsa_key = rsa::RsaPrivateKey::new(&mut rand::rngs::OsRng, RSA_KEY_BITS)
        .map_err(|error| failure(&error))?;
    let pkcs8_pem = rsa_key
        .to_pkcs8_pem(LineEnding::LF)
        .map_err(|error| failure(&error))?;
    let key_pair = rcgen::KeyPair::from_pem_and_sign_algo(&pkcs8_pem, &rcgen::PKCS_RSA_SHA256)
        .map_err(|error| failure(&error))?;
    let mut params = rcgen::CertificateParams::default();
    params.distinguished_name = rcgen::DistinguishedName::new();
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, COMMON_NAME);
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now;
    params.not_after = now + time::Duration::days(VALIDITY_DAYS);
    let certificate = params
        .self_signed(&key_pair)
        .map_err(|error| failure(&error))?;
    let private_key = rsa_key
        .to_pkcs1_pem(LineEnding::LF)
        .map_err(|error| failure(&error))?;
    Ok(TlsIdentityPem {
        certificate: certificate.pem(),
        private_key: private_key.to_string(),
    })
}

fn build_server_config(identity: &TlsIdentityPem) -> Result<rustls::ServerConfig, TlsSetupError> {
    let load = |error: &dyn std::fmt::Display| TlsSetupError::Load(error.to_string());
    let certificates: Vec<CertificateDer<'static>> =
        CertificateDer::pem_slice_iter(identity.certificate.as_bytes())
            .collect::<Result<_, _>>()
            .map_err(|error| load(&error))?;
    let private_key = PrivateKeyDer::from_pem_slice(identity.private_key.as_bytes())
        .map_err(|error| load(&error))?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| load(&error))?
        .with_no_client_auth()
        .with_single_cert(certificates, private_key)
        .map_err(|error| load(&error))
}

/// 앱이 고정하는 지문: 인증서 DER의 sha256, 콜론 없는 소문자 16진
pub fn certificate_fingerprint(identity: &TlsIdentityPem) -> Result<String, TlsSetupError> {
    let certificate = CertificateDer::from_pem_slice(identity.certificate.as_bytes())
        .map_err(|error| TlsSetupError::Load(error.to_string()))?;
    Ok(hex::encode(Sha256::digest(certificate.as_ref())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_pem_files_are_reused_byte_for_byte_and_keep_the_fingerprint() {
        let directory = tempfile::tempdir().unwrap();
        let tls_directory = directory.path().join("mobile-tls");
        let created = load_or_create_identity(&tls_directory).unwrap();
        let reloaded = load_or_create_identity(&tls_directory).unwrap();
        assert_eq!(created, reloaded);
        assert_eq!(
            certificate_fingerprint(&created).unwrap(),
            certificate_fingerprint(&reloaded).unwrap()
        );
        assert!(build_server_config(&reloaded).is_ok());
    }

    #[test]
    fn generated_files_use_private_modes_and_a_pkcs1_key() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let tls_directory = directory.path().join("mobile-tls");
        let identity = load_or_create_identity(&tls_directory).unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&tls_directory), 0o700);
        assert_eq!(mode(&tls_directory.join(PRIVATE_KEY_FILE_NAME)), 0o600);
        assert_eq!(mode(&tls_directory.join(CERTIFICATE_FILE_NAME)), 0o644);
        assert!(
            identity
                .private_key
                .starts_with("-----BEGIN RSA PRIVATE KEY-----")
        );
    }
}
