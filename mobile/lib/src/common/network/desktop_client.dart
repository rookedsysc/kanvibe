import 'dart:async';
import 'dart:convert';

import 'package:http/http.dart' as http;
import 'package:web_socket_channel/io.dart';

import '../../features/task_detail/domain/mirror_pane.dart';
import 'desktop_connection.dart';

/// 데스크탑의 `/api/mobile/*`와 이야기하는 유일한 통로.
///
/// 페어링만 토큰 없이 호출하고 나머지는 전부 토큰을 실어 보낸다.
/// 데스크탑에서 `/api/hooks/*`는 인증 없이 열려 있지만 이 클라이언트는 그쪽을 부르지 않는다.
class DesktopClient {
  DesktopClient({http.Client? httpClient})
    : _httpClient = httpClient ?? http.Client();

  final http.Client _httpClient;

  /// 사람이 기다려 줄 만한 시간. 넘어가면 주소가 틀렸을 가능성이 더 크다
  static const _requestTimeout = Duration(seconds: 8);

  /// 화면의 6자리 코드를 토큰으로 바꾼다. 성공하면 그 뒤로 코드는 필요 없다
  Future<String?> pair({
    required String host,
    required int port,
    required String code,
    required String deviceName,
  }) async {
    final response = await _httpClient
        .post(
          Uri.http('$host:$port', '/api/mobile/pair'),
          headers: {'Content-Type': 'application/json'},
          body: jsonEncode({'code': code, 'deviceName': deviceName}),
        )
        .timeout(_requestTimeout);

    if (response.statusCode != 200) {
      return null;
    }

    final body = jsonDecode(response.body) as Map<String, dynamic>;
    final token = body['token'];
    return token is String ? token : null;
  }

  /// 보드 전체를 한 번에 받는다
  Future<Map<String, dynamic>> fetchBoard(DesktopConnection connection) async {
    final response = await _httpClient
        .get(
          connection.buildHttpUri('/api/mobile/board'),
          headers: connection.authorizationHeaders,
        )
        .timeout(_requestTimeout);

    if (response.statusCode != 200) {
      throw DesktopRequestException(response.statusCode);
    }

    return (jsonDecode(response.body) as Map<String, dynamic>)['board']
        as Map<String, dynamic>;
  }

  /// 태스크의 탭과 pane 목록
  Future<TaskSurfaces> fetchSurfaces(
    DesktopConnection connection,
    String taskId,
  ) async {
    final response = await _httpClient
        .get(
          connection.buildHttpUri(
            '/api/mobile/tasks/${Uri.encodeComponent(taskId)}/surfaces',
          ),
          headers: connection.authorizationHeaders,
        )
        .timeout(_requestTimeout);

    /// 200도 409도 아니면 본문이 JSON이라는 보장이 없다. 상태를 먼저 걸러야
    /// 앞단이 끼워 넣은 HTML 오류 페이지가 [FormatException]으로 둔갑하지 않는다
    if (response.statusCode != 200 && response.statusCode != 409) {
      throw DesktopRequestException(response.statusCode);
    }

    final body = jsonDecode(response.body) as Map<String, dynamic>;

    /// 세션이 없거나 꺼진 것은 오류가 아니라 화면이 안내해야 하는 상태다
    if (response.statusCode == 409) {
      throw SurfaceUnavailableException(
        SurfaceUnavailableReason.fromWire(body['reason']?.toString()),
      );
    }

    return TaskSurfaces.fromJson(body['surfaces'] as Map<String, dynamic>);
  }

  /// pane 하나를 구독한다. 첫 프레임은 현재 화면이고 그 뒤로는 바뀌는 부분만 온다
  PaneStream openPaneStream(
    DesktopConnection connection,
    String taskId,
    String paneId,
  ) {
    final channel = IOWebSocketChannel.connect(
      connection.buildWebSocketUri('/api/mobile/stream', {
        'taskId': taskId,
        'paneId': paneId,
      }),
      headers: connection.authorizationHeaders,
    );

    return PaneStream(
      output: channel.stream.map((event) => event.toString()),
      onWrite: channel.sink.add,
      onClose: channel.sink.close,
    );
  }

  void close() => _httpClient.close();
}

/// 열려 있는 pane 구독 하나.
///
/// 소켓을 직접 들지 않고 흐름과 두 동작만 받는다. 터미널 위젯이 소켓 없이도 검증될 수 있어야 하기 때문이다.
class PaneStream {
  const PaneStream({
    required this.output,
    required this.onWrite,
    required this.onClose,
  });

  /// pane이 뱉는 바이트. 터미널이 그대로 먹는다
  final Stream<String> output;

  /// 모바일에서 누른 키를 pane으로 보낸다
  final void Function(String input) onWrite;

  final Future<void> Function() onClose;

  void write(String input) => onWrite(input);

  Future<void> close() => onClose();
}

/// 데스크탑이 200이 아닌 응답을 준 경우
class DesktopRequestException implements Exception {
  const DesktopRequestException(this.statusCode);

  final int statusCode;

  /// 토큰이 거절된 것과 그 밖의 실패는 화면에서 다르게 안내해야 한다
  bool get isUnauthorized => statusCode == 401;
}
