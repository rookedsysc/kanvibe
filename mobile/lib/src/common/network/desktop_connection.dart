/// 연결할 데스크탑 한 대. 사용자가 입력하는 것은 주소와 포트뿐이고 토큰은 페어링이 채운다
class DesktopConnection {
  const DesktopConnection({
    required this.host,
    required this.port,
    required this.token,
    this.name = '',
    this.certificateFingerprint,
  });

  final String host;
  final int port;
  final String token;
  final String name;

  /// 페어링 때 고정한 데스크탑 인증서의 지문. 없으면 TLS 도입 전에 페어링한 연결이라 평문으로 통신한다
  final String? certificateFingerprint;

  bool get isEncrypted => certificateFingerprint != null;

  String get id => '${host.trim().toLowerCase()}:$port';
  String get displayName => name.trim().isEmpty ? host : name.trim();
  String get address => '$host:$port';

  /// KanVibe가 패키지된 앱에서 여는 TLS 포트. 개발 빌드는 19746을 쓴다
  static const defaultPort = 9746;

  Uri buildHttpUri(String path, [Map<String, String>? query]) => isEncrypted
      ? Uri.https('$host:$port', path, query)
      : Uri.http('$host:$port', path, query);

  Uri buildWebSocketUri(String path, Map<String, String> query) => Uri(
    scheme: isEncrypted ? 'wss' : 'ws',
    host: host,
    port: port,
    path: path,
    queryParameters: query,
  );

  Map<String, String> get authorizationHeaders => {
    'Authorization': 'Bearer $token',
  };

  Map<String, dynamic> toJson() => {
    'host': host,
    'port': port,
    'token': token,
    'name': name,
    'certificateFingerprint': ?certificateFingerprint,
  };

  static DesktopConnection? fromJson(Map<String, dynamic> json) {
    final host = json['host'];
    final token = json['token'];
    final port = json['port'];

    if (host is! String || token is! String || port is! int) {
      return null;
    }
    final name = json['name'];
    if (name != null && name is! String) return null;
    final certificateFingerprint = json['certificateFingerprint'];
    if (certificateFingerprint != null && certificateFingerprint is! String) {
      return null;
    }
    return DesktopConnection(
      host: host,
      port: port,
      token: token,
      name: name as String? ?? '',
      certificateFingerprint: certificateFingerprint as String?,
    );
  }
}
