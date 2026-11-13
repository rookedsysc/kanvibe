import 'dart:io';

import 'package:crypto/crypto.dart';

/// 인증서 DER의 SHA-256을 소문자 hex로 만든다. 데스크탑 인증서는 자체 서명이라 체인 대신 이 값으로 신뢰한다
String computeCertificateFingerprint(X509Certificate certificate) =>
    sha256.convert(certificate.der).toString();

/// HTTP가 Authorization을 쓰기 전에 TLS peer의 지문을 확인한다.
/// 시스템 CA가 신뢰하는 인증서도 같은 지문 검사를 거친다.
HttpClient createPinnedHttpClient(
  String pinnedFingerprint, {
  Future<void>? cancelled,
}) {
  final client = HttpClient()..findProxy = (_) => 'DIRECT';
  client.connectionFactory = (uri, _, _) async {
    var isCancelled = false;
    ConnectionTask<SecureSocket>? task;
    SecureSocket? connectedSocket;
    cancelled?.then((_) {
      isCancelled = true;
      task?.cancel();
      connectedSocket?.destroy();
    });
    task = await SecureSocket.startConnect(
      uri.host,
      uri.port,
      // 자체 서명 인증서를 허용한 뒤 아래에서 모든 peer를 동일하게 고정 지문과 비교한다.
      onBadCertificate: (_) => true,
    );
    if (isCancelled) task.cancel();
    final socket = task.socket.then<Socket>((socket) {
      connectedSocket = socket;
      final certificate = socket.peerCertificate;
      if (isCancelled ||
          certificate == null ||
          computeCertificateFingerprint(certificate) != pinnedFingerprint) {
        socket.destroy();
        throw const HandshakeException(
          'TLS certificate pin mismatch or cancelled connection',
        );
      }
      return socket;
    });
    return ConnectionTask.fromSocket(socket, task.cancel);
  };
  return client;
}

/// 페어링 전용. 어떤 인증서든 받되 본 지문을 [onCertificate]로 알려, 페어링이 성공하면 그 값을 고정한다.
HttpClient createFingerprintCapturingHttpClient(
  void Function(String fingerprint) onCertificate,
) =>
    HttpClient(context: SecurityContext(withTrustedRoots: false))
      ..badCertificateCallback = (certificate, _, _) {
        onCertificate(computeCertificateFingerprint(certificate));
        return true;
      };
