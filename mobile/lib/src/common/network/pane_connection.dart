/// A가 인증한 task/pane 연결 한 번의 권한. 저장된 PC 페어링과 수명이 다르다.
class PaneConnection {
  const PaneConnection._({
    required this.grantId,
    required this.directUri,
    required this.token,
    required this.certificateFingerprint,
    required this.expiresAt,
  });

  final String grantId;
  final Uri directUri;
  final String token;
  final String certificateFingerprint;
  final num expiresAt;

  static PaneConnection fromJson(Object? value) {
    if (value is! Map<String, dynamic>) {
      throw const FormatException('터미널 연결 응답이 올바르지 않습니다.');
    }
    final direct = value['direct'];
    final grantId = value['grantId'];
    final expiresAt = value['expiresAt'];
    if (value['version'] != 1 ||
        grantId is! String ||
        !RegExp(r'^[A-Za-z0-9_-]{1,128}$').hasMatch(grantId) ||
        expiresAt is! num ||
        !expiresAt.isFinite ||
        expiresAt <= 0 ||
        direct is! Map<String, dynamic>) {
      throw const FormatException('터미널 연결 권한이 올바르지 않습니다.');
    }
    final host = direct['host'];
    final port = direct['port'];
    final path = direct['path'];
    final token = direct['token'];
    final fingerprint = direct['certificateFingerprint'];
    if (host is! String ||
        host.isEmpty ||
        host.contains(RegExp(r'[\x00-\x20\x7f\s/@?#\\]')) ||
        port is! int ||
        port < 1 ||
        port > 65535 ||
        path != '/stream/$grantId' ||
        token is! String ||
        !RegExp(r'^[A-Za-z0-9_-]{32,512}$').hasMatch(token) ||
        fingerprint is! String ||
        !RegExp(r'^[A-Fa-f0-9]{64}$').hasMatch(fingerprint)) {
      throw const FormatException('터미널 직접 연결 주소가 올바르지 않습니다.');
    }
    return PaneConnection._(
      grantId: grantId,
      directUri: Uri(
        scheme: 'wss',
        host: host,
        port: port,
        path: path as String,
      ),
      token: token,
      certificateFingerprint: fingerprint.toLowerCase(),
      // 만료 판단은 권한을 발급한 서버가 맡는다. 폰 시계가 빠르더라도 유효한 권한을 버리지 않는다.
      expiresAt: expiresAt,
    );
  }
}
