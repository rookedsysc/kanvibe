use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, RsaKeySize, PKCS_RSA_SHA256};
use std::{fs, io::Write, path::Path, sync::Arc};

pub struct Certificate {
    pub cert: Vec<u8>,
    pub key: Vec<u8>,
}
pub fn load_or_create(directory: &Path) -> Result<Certificate, String> {
    let certificate = directory.join("certificate.pem");
    let private_key = directory.join("private-key.pem");
    match (fs::read(&certificate), fs::read(&private_key)) {
        (Ok(cert), Ok(key)) => return Ok(Certificate { cert, key }),
        (Err(error), _) | (_, Err(error)) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err(error.to_string())
        }
        _ => {}
    }
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
    let key_pair = KeyPair::generate_rsa_for(&PKCS_RSA_SHA256, RsaKeySize::_2048)
        .map_err(|error| error.to_string())?;
    let mut params =
        CertificateParams::new(Vec::<String>::new()).map_err(|error| error.to_string())?;
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, "KanVibe Desktop");
    params.not_before = time::OffsetDateTime::now_utc();
    params.not_after = params.not_before + time::Duration::days(20 * 365);
    let cert = params
        .self_signed(&key_pair)
        .map_err(|error| error.to_string())?
        .pem()
        .into_bytes();
    let key = key_pair.serialize_pem().into_bytes();
    write_certificate_file(&private_key, &key, 0o600)?;
    write_certificate_file(&certificate, &cert, 0o644)?;
    Ok(Certificate { cert, key })
}
fn write_certificate_file(path: &Path, data: &[u8], mode: u32) -> Result<(), String> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    options
        .open(path)
        .and_then(|mut file| file.write_all(data))
        .map_err(|error| error.to_string())
}
pub fn config(certificate: &Certificate) -> Result<axum_server::tls_rustls::RustlsConfig, String> {
    let certs = rustls_pemfile::certs(&mut &certificate.cert[..])
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let key = rustls_pemfile::private_key(&mut &certificate.key[..])
        .map_err(|error| error.to_string())?
        .ok_or("TLS 개인 키가 없습니다")?;
    let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|error| error.to_string())?
    .with_no_client_auth()
    .with_single_cert(certs, key)
    .map_err(|error| error.to_string())?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(axum_server::tls_rustls::RustlsConfig::from_config(
        Arc::new(config),
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persistent_certificate_reuses_fingerprint_key_and_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let first = load_or_create(root.path()).unwrap();
        let second = load_or_create(root.path()).unwrap();
        assert_eq!(first.cert, second.cert);
        assert_eq!(first.key, second.key);
        use sha2::{Digest, Sha256};
        let digest = |certificate: &Certificate| {
            let der = rustls_pemfile::certs(&mut &certificate.cert[..])
                .next()
                .unwrap()
                .unwrap();
            hex::encode(Sha256::digest(der.as_ref()))
        };
        assert_eq!(digest(&first), digest(&second));
        assert_eq!(digest(&first).len(), 64);
        assert_eq!(
            fs::metadata(root.path().join("private-key.pem"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(config(&first).is_ok());
    }
    #[test]
    fn damaged_complete_pair_is_not_replaced_but_missing_key_regenerates_pair() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("certificate.pem"), "corrupt").unwrap();
        fs::write(root.path().join("private-key.pem"), "corrupt-key").unwrap();
        let broken = load_or_create(root.path()).unwrap();
        assert!(config(&broken).is_err());
        assert_eq!(
            fs::read_to_string(root.path().join("certificate.pem")).unwrap(),
            "corrupt"
        );
        fs::remove_file(root.path().join("private-key.pem")).unwrap();
        let repaired = load_or_create(root.path()).unwrap();
        assert!(config(&repaired).is_ok());
        assert_ne!(repaired.cert, b"corrupt");
    }
}
