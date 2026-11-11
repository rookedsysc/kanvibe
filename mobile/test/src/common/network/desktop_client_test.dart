import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_command.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/features/task_create/domain/session_type.dart';
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
  defineTlsPinningTests();

  test(
    'terminal shortcut uses authenticated pane-scoped POST and reads screen',
    () async {
      final client = DesktopClient(
        httpClient: MockClient((request) async {
          expect(request.method, 'POST');
          expect(request.url.pathSegments, [
            'api',
            'mobile',
            'tasks',
            'task-1',
            'panes',
            '%19',
            'shortcuts',
          ]);
          expect(request.headers['authorization'], 'Bearer test-token');
          expect(jsonDecode(request.body), {'action': 'copy-mode'});
          return http.Response(
            jsonEncode({
              'surfaces': {'sessionType': 'tmux', 'tabs': []},
              'screen': 'copy viewport',
            }),
            200,
          );
        }),
      );
      final result = await client.runTerminalShortcut(
        _connection,
        'task-1',
        '%19',
        'copy-mode',
      );
      expect(result.screen, 'copy viewport');
      expect(result.surfaces.tabs, isEmpty);
    },
  );

  test(
    'shortcut HTTP errors do not attempt to decode non-JSON bodies',
    () async {
      final client = clientReplying(404, '<html>old desktop</html>');
      expect(
        client.runTerminalShortcut(_connection, 'task-1', '%19', 'copy-mode'),
        throwsA(isA<DesktopRequestException>()),
      );
    },
  );

  test('터미널 탭 추가는 인증 POST를 보내고 새 pane과 갱신 목록을 받는다', () async {
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        expect(request.method, 'POST');
        expect(request.url.path, '/api/mobile/tasks/task-1/tabs');
        expect(request.headers['authorization'], 'Bearer test-token');
        expect(request.body, isEmpty);
        return http.Response(
          jsonEncode({
            'success': true,
            'paneId': '%20',
            'surfaces': {
              'sessionType': 'tmux',
              'canResize': true,
              'tabs': [
                {
                  'id': '@2',
                  'panes': [
                    {'id': '%20', 'width': 80, 'height': 24},
                  ],
                },
              ],
            },
          }),
          200,
        );
      }),
    );
    addTearDown(client.close);
    final created = await client.createTerminalTab(_connection, 'task-1');
    expect(created.paneId, '%20');
    expect(created.surfaces.allPanes.single.id, '%20');
    expect(created.surfaces.canResize, isTrue);
  });

  test('탭 추가 409는 사용 불가 이유를 보존한다', () async {
    final client = clientReplying(409, '{"reason":"no-session"}');
    addTearDown(client.close);
    await expectLater(
      client.createTerminalTab(_connection, 'task-1'),
      throwsA(
        isA<SurfaceUnavailableException>().having(
          (e) => e.reason,
          'reason',
          SurfaceUnavailableReason.noSession,
        ),
      ),
    );
  });

  for (final status in [401, 500]) {
    test('탭 추가 $status의 HTML 오류도 상태 코드로 전달한다', () async {
      final client = clientReplying(status, '<html>upstream failure</html>');
      addTearDown(client.close);
      await expectLater(
        client.createTerminalTab(_connection, 'task-1'),
        throwsA(
          isA<DesktopRequestException>().having(
            (e) => e.statusCode,
            'statusCode',
            status,
          ),
        ),
      );
    });
  }

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

  test('세션 종류 변경은 인증 PATCH로 새 종류와 기존 세션 닫기 여부를 보낸다', () async {
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        expect(request.method, 'PATCH');
        expect(request.url.path, '/api/mobile/tasks/task-1/session-type');
        expect(request.headers['authorization'], 'Bearer test-token');
        expect(jsonDecode(request.body), {
          'sessionType': 'zellij',
          'shouldCloseExistingSession': true,
        });
        return http.Response(jsonEncode({'success': true, 'task': {}}), 200);
      }),
    );
    addTearDown(client.close);

    await client.changeTaskSessionType(
      _connection,
      'task-1',
      sessionType: SessionType.zellij,
      shouldCloseExistingSession: true,
    );
  });

  test('세션 종류 변경을 데스크탑이 거절하면 사유를 담아 던진다', () async {
    final client = DesktopClient(
      httpClient: MockClient(
        (request) async => http.Response.bytes(
          utf8.encode(
            jsonEncode({'success': false, 'error': 'zellij이(가) 설치되어 있지 않습니다.'}),
          ),
          409,
          headers: {'content-type': 'application/json; charset=utf-8'},
        ),
      ),
    );
    addTearDown(client.close);

    await expectLater(
      client.changeTaskSessionType(
        _connection,
        'task-1',
        sessionType: SessionType.zellij,
        shouldCloseExistingSession: false,
      ),
      throwsA(
        isA<DesktopRequestException>()
            .having((error) => error.statusCode, 'statusCode', 409)
            .having((error) => error.message, 'message', contains('zellij')),
      ),
    );
  });

  test('기존 세션이 켜져 있는지 인증 GET으로 묻는다', () async {
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        expect(request.method, 'GET');
        expect(request.url.path, '/api/mobile/tasks/task-1/session-alive');
        expect(request.headers['authorization'], 'Bearer test-token');
        return http.Response(
          jsonEncode({'success': true, 'isAlive': true}),
          200,
        );
      }),
    );
    addTearDown(client.close);

    expect(await client.fetchTaskSessionAlive(_connection, 'task-1'), isTrue);
  });

  group('소켓 명령', () {
    late HttpServer server;
    late DesktopClient client;
    late DesktopConnection connection;
    late void Function(WebSocket socket, Map<String, dynamic> message)
    onMessage;
    List<String>? advertisedCommands;
    final sockets = <WebSocket>[];

    setUp(() async {
      advertisedCommands = ['delete-task', 'get-tab-close-info', 'close-tab'];
      onMessage = (socket, message) {};
      sockets.clear();
      server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      server.listen((request) async {
        final socket = await WebSocketTransformer.upgrade(request);
        sockets.add(socket);
        socket.add(
          jsonEncode({'type': 'connected', 'commands': ?advertisedCommands}),
        );
        socket.listen(
          (data) => onMessage(
            socket,
            jsonDecode(data as String) as Map<String, dynamic>,
          ),
        );
      });
      client = DesktopClient();
      connection = DesktopConnection(
        host: '127.0.0.1',
        port: server.port,
        token: 'test-token',
      );
    });

    tearDown(() async {
      client.close();
      await server.close(force: true);
    });

    /// 이벤트 스트림을 열고 `connected`가 도착해 기능 목록이 기록될 때까지 기다린다
    Future<({DesktopEventStream stream, List<Map<String, dynamic>> events})>
    openStream() async {
      final stream = client.openEventStream(connection);
      addTearDown(stream.close);
      final events = <Map<String, dynamic>>[];
      final connected = Completer<void>();
      stream.events.listen((event) {
        events.add(event);
        if (event['type'] == 'connected') connected.complete();
      });
      await connected.future;
      return (stream: stream, events: events);
    }

    test('requestId로 짝지어 성공 응답의 값을 돌려주고 이벤트로는 흘리지 않는다', () async {
      onMessage = (socket, message) {
        expect(message['type'], 'command');
        expect(message['command'], 'delete-task');
        expect(message['taskId'], 'task-1');
        expect(message['isForced'], isTrue);
        socket.add(
          jsonEncode({
            'type': 'command-result',
            'requestId': message['requestId'],
            'ok': true,
            'failedPeerCount': 2,
          }),
        );
      };
      final opened = await openStream();

      final result = await client.runCommand(
        connection,
        DesktopCommand.deleteTask('task-1', isForced: true),
      );

      expect(result['failedPeerCount'], 2);
      expect(opened.events.map((event) => event['type']), ['connected']);
    });

    test('동시에 보낸 명령은 서로의 응답과 섞이지 않는다', () async {
      final received = <Map<String, dynamic>>[];
      onMessage = (socket, message) {
        received.add(message);
        if (received.length < 2) return;
        // 나중에 온 명령부터 답해 순서가 아니라 requestId로 짝지어지는지 본다
        for (final request in received.reversed) {
          socket.add(
            jsonEncode({
              'type': 'command-result',
              'requestId': request['requestId'],
              'ok': true,
              'processNames': [request['paneId']],
            }),
          );
        }
      };
      await openStream();

      final results = await Future.wait([
        client.runCommand(
          connection,
          DesktopCommand.getTabCloseInfo('task-1', '%1'),
        ),
        client.runCommand(
          connection,
          DesktopCommand.getTabCloseInfo('task-1', '%2'),
        ),
      ]);

      expect(results[0]['processNames'], ['%1']);
      expect(results[1]['processNames'], ['%2']);
    });

    test('실패 응답은 코드와 한국어 사유를 담은 예외로 던진다', () async {
      onMessage = (socket, message) {
        socket.add(
          jsonEncode({
            'type': 'command-result',
            'requestId': message['requestId'],
            'ok': false,
            'code': 'last-tab',
            'error': '마지막 탭은 닫을 수 없습니다',
          }),
        );
      };
      await openStream();

      await expectLater(
        client.runCommand(connection, DesktopCommand.closeTab('task-1', '%1')),
        throwsA(
          isA<DesktopCommandFailure>()
              .having((error) => error.code, 'code', 'last-tab')
              .having((error) => error.message, 'message', '마지막 탭은 닫을 수 없습니다'),
        ),
      );
    });

    test('unsupported-command 응답은 데스크탑 업데이트 필요로 바꾼다', () async {
      onMessage = (socket, message) {
        socket.add(
          jsonEncode({
            'type': 'command-result',
            'requestId': message['requestId'],
            'ok': false,
            'code': 'unsupported-command',
            'error': '이 데스크탑이 지원하지 않는 명령입니다',
          }),
        );
      };
      await openStream();

      await expectLater(
        client.runCommand(connection, DesktopCommand.closeTab('task-1', '%1')),
        throwsA(isA<DesktopUpdateRequiredException>()),
      );
    });

    test('connected에 commands가 없으면 아무것도 보내지 않고 업데이트 필요로 실패한다', () async {
      advertisedCommands = null;
      var receivedCount = 0;
      onMessage = (socket, message) => receivedCount++;
      await openStream();

      await expectLater(
        client.runCommand(connection, DesktopCommand.closeTab('task-1', '%1')),
        throwsA(isA<DesktopUpdateRequiredException>()),
      );
      expect(receivedCount, 0);
    });

    test('등록 제거 명령을 모르는 데스크탑에는 일반 삭제를 대신 보내지 않는다', () async {
      advertisedCommands = ['delete-task'];
      var receivedCount = 0;
      onMessage = (socket, message) => receivedCount++;
      await openStream();
      await expectLater(
        client.runCommand(
          connection,
          DesktopCommand.removeTaskRegistration('task-1'),
        ),
        throwsA(isA<DesktopUpdateRequiredException>()),
      );
      expect(receivedCount, 0);
    });

    test('등록 제거는 파일 삭제 옵션 없는 별도 명령으로 직렬화한다', () async {
      advertisedCommands = ['remove-task-registration'];
      Map<String, dynamic>? received;
      onMessage = (socket, message) {
        received = message;
        socket.add(
          jsonEncode({
            'type': 'command-result',
            'requestId': message['requestId'],
            'ok': true,
            'failedPeerCount': 0,
          }),
        );
      };
      await openStream();
      await client.runCommand(
        connection,
        DesktopCommand.removeTaskRegistration('task-1'),
      );
      expect(received, containsPair('command', 'remove-task-registration'));
      expect(received, containsPair('taskId', 'task-1'));
      expect(received, isNot(contains('isForced')));
    });

    test('commands에 없는 명령은 보내지 않고 업데이트 필요로 실패한다', () async {
      advertisedCommands = ['delete-task'];
      var receivedCount = 0;
      onMessage = (socket, message) => receivedCount++;
      await openStream();

      await expectLater(
        client.runCommand(connection, DesktopCommand.closeTab('task-1', '%1')),
        throwsA(isA<DesktopUpdateRequiredException>()),
      );
      expect(receivedCount, 0);
    });

    test('열린 이벤트 스트림이 없으면 즉시 연결 끊김으로 실패한다', () async {
      await expectLater(
        client.runCommand(connection, DesktopCommand.closeTab('task-1', '%1')),
        throwsA(isA<DesktopDisconnectedException>()),
      );
    });

    test('스트림을 닫으면 그 연결의 명령은 다시 연결 끊김으로 실패한다', () async {
      final opened = await openStream();
      await opened.stream.close();

      await expectLater(
        client.runCommand(connection, DesktopCommand.closeTab('task-1', '%1')),
        throwsA(isA<DesktopDisconnectedException>()),
      );
    });

    test('응답을 기다리는 중 소켓이 닫히면 연결 끊김으로 실패한다', () async {
      onMessage = (socket, message) => socket.close();
      await openStream();

      await expectLater(
        client.runCommand(connection, DesktopCommand.closeTab('task-1', '%1')),
        throwsA(isA<DesktopDisconnectedException>()),
      );
    });

    test('응답이 오지 않으면 명령별 제한 시간 뒤에 실패한다', () async {
      await openStream();

      await expectLater(
        client.runCommand(
          connection,
          DesktopCommand.getTabCloseInfo('task-1', '%1'),
          timeout: const Duration(milliseconds: 50),
        ),
        throwsA(isA<TimeoutException>()),
      );
    });
  });
}

/// 데스크탑과 같은 자체 서명 인증서를 띄운다. 개인키를 저장소에 두지 않으려고 테스트마다 만든다
Future<({SecurityContext context, String fingerprint})> createSelfSignedTls(
  String commonName,
) async {
  final directory = await Directory.systemTemp.createTemp('kanvibe-tls-');
  addTearDown(() => directory.delete(recursive: true));
  final keyPath = '${directory.path}/key.pem';
  final certificatePath = '${directory.path}/cert.pem';
  final generated = await Process.run('openssl', [
    'req',
    '-x509',
    '-newkey',
    'rsa:2048',
    '-nodes',
    '-days',
    '1',
    '-subj',
    '/CN=$commonName',
    '-keyout',
    keyPath,
    '-out',
    certificatePath,
  ]);
  expect(generated.exitCode, 0, reason: '${generated.stderr}');
  final fingerprintOutput = await Process.run('openssl', [
    'x509',
    '-in',
    certificatePath,
    '-noout',
    '-fingerprint',
    '-sha256',
  ]);
  final fingerprint = (fingerprintOutput.stdout as String)
      .split('=')
      .last
      .trim()
      .replaceAll(':', '')
      .toLowerCase();
  return (
    context: SecurityContext()
      ..useCertificateChain(certificatePath)
      ..usePrivateKey(keyPath),
    fingerprint: fingerprint,
  );
}

void defineTlsPinningTests() {
  test('페어링은 TLS로 보내고 그때 본 인증서 지문을 돌려준다', () async {
    final tls = await createSelfSignedTls('desktop');
    final server = await HttpServer.bindSecure(
      InternetAddress.loopbackIPv4,
      0,
      tls.context,
    );
    addTearDown(() => server.close(force: true));
    server.listen((request) async {
      expect(request.uri.path, '/api/mobile/pair');
      request.response.write(
        jsonEncode({'success': true, 'token': 'new-token'}),
      );
      await request.response.close();
    });
    final client = DesktopClient();
    addTearDown(client.close);

    final result = await client.pair(
      host: '127.0.0.1',
      port: server.port,
      code: '123456',
      deviceName: 'phone',
    );

    expect(result?.token, 'new-token');
    expect(result?.certificateFingerprint, tls.fingerprint);
  });

  test('고정한 지문과 같은 인증서에는 HTTP와 WebSocket 모두 연결한다', () async {
    final tls = await createSelfSignedTls('desktop');
    final server = await HttpServer.bindSecure(
      InternetAddress.loopbackIPv4,
      0,
      tls.context,
    );
    addTearDown(() => server.close(force: true));
    server.listen((request) async {
      if (WebSocketTransformer.isUpgradeRequest(request)) {
        final socket = await WebSocketTransformer.upgrade(request);
        socket.add(jsonEncode({'type': 'ready'}));
        await socket.close();
        return;
      }
      request.response.write(jsonEncode({'tasks': <Object>[]}));
      await request.response.close();
    });
    final client = DesktopClient();
    addTearDown(client.close);
    final connection = DesktopConnection(
      host: '127.0.0.1',
      port: server.port,
      token: 'token',
      certificateFingerprint: tls.fingerprint,
    );

    expect(connection.buildHttpUri('/x').scheme, 'https');
    await client.unpairDevice(connection);
    final stream = client.openEventStream(connection);
    addTearDown(stream.close);
    expect(await stream.events.toList(), [
      {'type': 'ready'},
    ]);
  });

  test('고정한 지문과 다른 인증서면 토큰을 보내기 전에 연결을 끊는다', () async {
    final tls = await createSelfSignedTls('impostor');
    final server = await HttpServer.bindSecure(
      InternetAddress.loopbackIPv4,
      0,
      tls.context,
    );
    addTearDown(() => server.close(force: true));
    var receivedRequestCount = 0;
    server.listen((request) async {
      receivedRequestCount += 1;
      await request.response.close();
    }, onError: (_) {});
    final client = DesktopClient();
    addTearDown(client.close);
    final connection = DesktopConnection(
      host: '127.0.0.1',
      port: server.port,
      token: 'secret-token',
      certificateFingerprint: 'a' * 64,
    );

    await expectLater(client.unpairDevice(connection), throwsA(anything));
    final stream = client.openEventStream(connection);
    addTearDown(stream.close);
    await expectLater(stream.events.drain<void>(), throwsA(anything));
    expect(receivedRequestCount, 0);
  });

  test('지문을 저장하고 복원한다. 지문 없는 옛 연결은 평문으로 남는다', () {
    const pinned = DesktopConnection(
      host: 'pc',
      port: 9746,
      token: 't',
      certificateFingerprint: 'abc',
    );
    final restored = DesktopConnection.fromJson(pinned.toJson());
    expect(restored?.certificateFingerprint, 'abc');
    expect(restored?.buildWebSocketUri('/s', const {}).scheme, 'wss');

    final legacy = DesktopConnection.fromJson({
      'host': 'pc',
      'port': 9736,
      'token': 't',
    });
    expect(legacy?.isEncrypted, isFalse);
    expect(legacy?.buildHttpUri('/x').scheme, 'http');
  });
}
