/// 연결할 데스크탑 한 대. 사용자가 입력하는 것은 주소와 포트뿐이고 토큰은 페어링이 채운다
class DesktopConnection {
  const DesktopConnection({
    required this.host,
    required this.port,
    required this.token,
  });

  final String host;
  final int port;
  final String token;

  /// KanVibe가 패키지된 앱에서 여는 포트. 개발 빌드는 19736을 쓴다
  static const defaultPort = 9736;

  Uri buildHttpUri(String path, [Map<String, String>? query]) =>
      Uri.http('$host:$port', path, query);

  Uri buildWebSocketUri(String path, Map<String, String> query) => Uri(
    scheme: 'ws',
    host: host,
    port: port,
    path: path,
    queryParameters: query,
  );

  Map<String, String> get authorizationHeaders => {
    'Authorization': 'Bearer $token',
  };

  Map<String, dynamic> toJson() => {'host': host, 'port': port, 'token': token};

  static DesktopConnection? fromJson(Map<String, dynamic> json) {
    final host = json['host'];
    final token = json['token'];
    final port = json['port'];

    if (host is! String || token is! String || port is! int) {
      return null;
    }
    return DesktopConnection(host: host, port: port, token: token);
  }
}
