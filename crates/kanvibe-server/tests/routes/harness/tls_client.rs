//! 모바일 앱처럼 인증서 DER의 sha256 지문으로 서버를 믿는 TLS 클라이언트

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use kanvibe_server::tls::{
    CERTIFICATE_FILE_NAME, PRIVATE_KEY_FILE_NAME, TlsIdentityPem, certificate_fingerprint,
};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use sha2::{Digest, Sha256};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

#[derive(Debug)]
struct PinnedCertificate {
    fingerprint: String,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PinnedCertificate {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let fingerprint = hex::encode(Sha256::digest(end_entity.as_ref()));
        if fingerprint == self.fingerprint {
            return Ok(ServerCertVerified::assertion());
        }
        Err(rustls::Error::General("인증서 지문이 다르다".to_string()))
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            certificate,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            certificate,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

pub async fn connect_tls(address: SocketAddr, fingerprint: &str) -> TlsStream<TcpStream> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = PinnedCertificate {
        fingerprint: fingerprint.to_string(),
        provider: provider.clone(),
    };
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("TLS 버전")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    let tcp = TcpStream::connect(address).await.expect("TLS 포트에 연결");
    let server_name = ServerName::IpAddress(address.ip().into());
    tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(server_name, tcp)
        .await
        .expect("TLS 핸드셰이크")
}

/// RSA 생성은 디버그 빌드에서 수 초가 걸려, 서버를 띄우는 테스트는 빠른 ECDSA 인증서를 미리 써 둔다.
/// 서버는 두 파일이 다 있으면 그대로 읽으므로 생성 경로를 건너뛴다
pub fn write_test_identity(directory: &Path) -> String {
    let key_pair = rcgen::KeyPair::generate().expect("ECDSA 키");
    let params = rcgen::CertificateParams::new(vec!["127.0.0.1".to_string()]).expect("인증서 설정");
    let certificate = params.self_signed(&key_pair).expect("자체 서명");
    let identity = TlsIdentityPem {
        certificate: certificate.pem(),
        private_key: key_pair.serialize_pem(),
    };
    std::fs::create_dir_all(directory).expect("인증서 폴더");
    std::fs::write(directory.join(CERTIFICATE_FILE_NAME), &identity.certificate)
        .expect("인증서 쓰기");
    std::fs::write(directory.join(PRIVATE_KEY_FILE_NAME), &identity.private_key).expect("키 쓰기");
    certificate_fingerprint(&identity).expect("지문")
}
