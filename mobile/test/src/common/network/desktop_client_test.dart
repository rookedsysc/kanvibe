import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/features/task_detail/domain/mirror_pane.dart';

const _connection = DesktopConnection(
  host: '127.0.0.1',
  port: 9736,
  token: 'test-token',
);

DesktopClient clientReplying(int statusCode, String body) => DesktopClient(
  httpClient: MockClient((_) async => http.Response(body, statusCode)),
);

void main() {
  test('상태 이벤트는 pane과 분리한 인증 WebSocket으로 구독한다', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    const event = {
      'type': 'task-status-changed',
      'eventId': 'event-1',
      'taskId': 'task-1',
      'status': 'review',
    };
    server.listen((request) async {
      expect(request.uri.path, '/api/mobile/events');
      expect(request.uri.queryParameters, isEmpty);
      expect(request.headers.value('authorization'), 'Bearer test-token');
      final socket = await WebSocketTransformer.upgrade(request);
      socket.add(jsonEncode(event));
      await socket.close(deviceUnpairedCloseCode, 'Device unpaired');
    });
    final client = DesktopClient();
    addTearDown(client.close);
    final stream = client.openEventStream(
      DesktopConnection(
        host: '127.0.0.1',
        port: server.port,
        token: 'test-token',
      ),
    );
    addTearDown(stream.close);

    expect(await stream.events.toList(), [event]);
    expect(stream.readCloseCode(), deviceUnpairedCloseCode);
  });

  test('이벤트 소켓 handshake 거절도 스트림 오류로 전달한다', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    server.listen((request) async {
      request.response.statusCode = 401;
      await request.response.close();
    });
    final client = DesktopClient();
    addTearDown(client.close);
    final stream = client.openEventStream(
      DesktopConnection(
        host: '127.0.0.1',
        port: server.port,
        token: 'revoked-token',
      ),
    );

    await expectLater(stream.events.drain<void>(), throwsA(isA<Exception>()));
    expect(stream.readCloseCode(), isNull);
  });

  test('푸시 토큰은 연결 식별자와 플랫폼을 담아 인증 등록하고 해제한다', () async {
    final requests = <http.Request>[];
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        requests.add(request);
        return http.Response('{}', 200);
      }),
    );
    addTearDown(client.close);

    await client.registerNotifications(
      _connection,
      pushToken: 'fcm-token',
      platform: 'android',
    );
    await client.unregisterNotifications(_connection);

    expect(requests.map((request) => request.method), ['PUT', 'DELETE']);
    expect(requests.first.url.path, '/api/mobile/notifications');
    expect(requests.last.url.path, '/api/mobile/notifications');
    expect(
      requests.every(
        (request) => request.headers['authorization'] == 'Bearer test-token',
      ),
      isTrue,
    );
    expect(jsonDecode(requests.first.body), {
      'pushToken': 'fcm-token',
      'platform': 'android',
      'connectionId': _connection.id,
    });
  });

  test('알림 대상 태스크를 인증 조회하며 삭제된 태스크는 null로 돌려준다', () async {
    final requests = <http.Request>[];
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        requests.add(request);
        return request.url.path.endsWith('/missing')
            ? http.Response('not found', 404)
            : http.Response('{"task":{"id":"task-1","status":"review"}}', 200);
      }),
    );
    addTearDown(client.close);

    expect(await client.fetchTask(_connection, 'task-1'), {
      'id': 'task-1',
      'status': 'review',
    });
    expect(await client.fetchTask(_connection, 'missing'), isNull);
    expect(requests.first.url.path, '/api/mobile/tasks/task-1');
    expect(requests.first.headers['authorization'], 'Bearer test-token');
  });

  test('알림 등록과 단일 태스크 조회도 인증 거절 상태를 보존한다', () async {
    final client = clientReplying(401, '<html>Unauthorized</html>');
    addTearDown(client.close);
    final unauthorized = throwsA(
      isA<DesktopRequestException>().having(
        (error) => error.isUnauthorized,
        'isUnauthorized',
        isTrue,
      ),
    );

    await expectLater(
      client.registerNotifications(
        _connection,
        pushToken: 'token',
        platform: 'ios',
      ),
      unauthorized,
    );
    await expectLater(
      client.unregisterNotifications(_connection),
      unauthorized,
    );
    await expectLater(client.fetchTask(_connection, 'task-1'), unauthorized);
  });

  test('실제 WebSocket에서 크기는 binary, 입력은 text 프레임으로 전송한다', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    final received = <dynamic>[];
    final messages = Completer<void>();
    server.listen((request) async {
      expect(request.headers.value('authorization'), 'Bearer test-token');
      final socket = await WebSocketTransformer.upgrade(request);
      socket.listen((event) {
        received.add(event);
        if (received.length == 2) {
          socket.add('terminal-output');
          messages.complete();
        }
      });
    });
    final client = DesktopClient();
    addTearDown(client.close);
    final stream = client.openPaneStream(
      DesktopConnection(
        host: '127.0.0.1',
        port: server.port,
        token: 'test-token',
      ),
      'task-1',
      '%19',
    );
    addTearDown(stream.close);
    final output = stream.output.first;
    stream.resize(44, 70);
    const text = '{"type":"resize","cols":10,"rows":5}';
    stream.write(text);
    await messages.future.timeout(const Duration(seconds: 5));
    expect(received.first, isA<List<int>>());
    expect(jsonDecode(utf8.decode(received.first as List<int>)), {
      'type': 'resize',
      'cols': 44,
      'rows': 70,
    });
    expect(received.last, text);
    expect(await output, 'terminal-output');
  });

  test('앞단이 끼워 넣은 HTML 오류도 상태 코드로 알린다', () async {
    /// 본문을 먼저 읽으면 FormatException이 되어, 화면이 사유별 안내를 고를 기회를 잃는다
    final client = clientReplying(502, '<html>Bad Gateway</html>');

    await expectLater(
      client.fetchSurfaces(_connection, 'task-1'),
      throwsA(
        isA<DesktopRequestException>().having(
          (error) => error.statusCode,
          'statusCode',
          502,
        ),
      ),
    );
  });

  test('토큰이 거절되면 401을 그대로 올려 보낸다', () async {
    final client = clientReplying(401, '');

    await expectLater(
      client.fetchSurfaces(_connection, 'task-1'),
      throwsA(
        isA<DesktopRequestException>().having(
          (error) => error.isUnauthorized,
          'isUnauthorized',
          isTrue,
        ),
      ),
    );
  });

  test('409는 화면이 안내할 사유로 바꾼다', () async {
    final client = clientReplying(409, '{"reason":"session-not-running"}');

    await expectLater(
      client.fetchSurfaces(_connection, 'task-1'),
      throwsA(
        isA<SurfaceUnavailableException>().having(
          (error) => error.reason,
          'reason',
          SurfaceUnavailableReason.sessionNotRunning,
        ),
      ),
    );
  });

  test('200이면 탭과 pane을 읽어 온다', () async {
    final client = clientReplying(
      200,
      '{"surfaces":{"tabs":[{"id":"@10","name":"kanvibe","panes":[]}]}}',
    );

    final surfaces = await client.fetchSurfaces(_connection, 'task-1');

    expect(surfaces.tabs.single.name, 'kanvibe');
  });
}
