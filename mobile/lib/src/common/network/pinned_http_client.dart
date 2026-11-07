import 'dart:io';

import 'package:crypto/crypto.dart';

/// 인증서 DER의 SHA-256을 소문자 hex로 만든다. 데스크탑 인증서는 자체 서명이라 체인 대신 이 값으로 신뢰한다
String computeCertificateFingerprint(X509Certificate certificate) =>
    sha256.convert(certificate.der).toString();

/// 지문이 [pinnedFingerprint]와 같은 인증서만 받는 클라이언트.
///
/// 신뢰 루트를 비워 두어 모든 인증서가 [HttpClient.badCertificateCallback]을 거치게 한다.
/// 시스템이 신뢰하는 인증서라도 지문이 다르면 거절해야 고정의 의미가 있다.
HttpClient createPinnedHttpClient(String pinnedFingerprint) =>
    HttpClient(context: SecurityContext(withTrustedRoots: false))
      ..badCertificateCallback = (certificate, _, _) =>
          computeCertificateFingerprint(certificate) == pinnedFingerprint;

/// 페어링 전용. 어떤 인증서든 받되 본 지문을 [onCertificate]로 알려, 페어링이 성공하면 그 값을 고정한다.
HttpClient createFingerprintCapturingHttpClient(
  void Function(String fingerprint) onCertificate,
) =>
    HttpClient(context: SecurityContext(withTrustedRoots: false))
      ..badCertificateCallback = (certificate, _, _) {
        onCertificate(computeCertificateFingerprint(certificate));
        return true;
      };
