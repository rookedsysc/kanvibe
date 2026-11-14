import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/network/header_close_retry_client.dart';

const closedBeforeHeaders = 'Connection closed before full header was received';
final uri = Uri.parse(
  'https://desktop.test:9746/api/mobile/diff-stats?offset=0',
);
const connection = DesktopConnection(
  host: 'desktop.test',
  port: 9746,
  token: 'fixture-token',
  certificateFingerprint: 'fixture-fingerprint',
);

void main() {
  for (final method in ['GET', 'HEAD']) {
    test('$method 빈 본문은 헤더 전 연결 종료 후 새 요청으로 한 번 성공한다', () async {
      final requests = <http.BaseRequest>[];
      final client = HeaderCloseRetryClient(
        MockClient.streaming((request, body) async {
          requests.add(request);
          await body.drain<void>();
          if (requests.length == 1) {
            throw http.ClientException(closedBeforeHeaders, request.url);
          }
          return http.StreamedResponse(Stream.value(utf8.encode('ok')), 200);
        }),
      );
      addTearDown(client.close);
      final request = http.Request(method, uri)
        ..headers.addAll({
          'Authorization': 'Bearer fixture-token',
          'Accept': 'application/json',
        })
        ..followRedirects = false
        ..maxRedirects = 2
        ..persistentConnection = false;
      final response = await http.Response.fromStream(
        await client.send(request),
      );
      expect(response.statusCode, 200);
      expect(requests.length, 2);
      expect(identical(requests.first, requests.last), isFalse);
      expect(requests.last.url, uri);
      expect(requests.last.method, method);
      expect(requests.last.headers, requests.first.headers);
      expect(requests.last.followRedirects, isFalse);
      expect(requests.last.maxRedirects, 2);
      expect(requests.last.persistentConnection, isFalse);
    });
  }

  test('같은 연결 종료가 두 번 나면 원래 오류를 전파하고 세 번째로 보내지 않는다', () async {
    var attempts = 0;
    final error = http.ClientException(closedBeforeHeaders, uri);
    final client = HeaderCloseRetryClient(
      MockClient((_) async {
        attempts++;
        throw error;
      }),
    );
    addTearDown(client.close);
    await expectLater(client.get(uri), throwsA(same(error)));
    expect(attempts, 2);
  });

  for (final method in ['POST', 'PUT', 'PATCH', 'DELETE']) {
    for (final withBody in [false, true]) {
      test('$method body=$withBody 변경 요청은 한 번만 전송한다', () async {
        var attempts = 0;
        final error = http.ClientException(closedBeforeHeaders, uri);
        final client = HeaderCloseRetryClient(
          MockClient((_) async {
            attempts++;
            throw error;
          }),
        );
        addTearDown(client.close);
        final request = http.Request(method, uri);
        if (withBody) request.body = '{"value":1}';
        await expectLater(client.send(request), throwsA(same(error)));
        expect(attempts, 1);
      });
    }
  }

  test('GET에도 본문이 있으면 재전송하지 않는다', () async {
    var attempts = 0;
    final client = HeaderCloseRetryClient(
      MockClient((_) async {
        attempts++;
        throw http.ClientException(closedBeforeHeaders, uri);
      }),
    );
    addTearDown(client.close);
    await expectLater(
      client.send(http.Request('GET', uri)..body = 'payload'),
      throwsA(isA<http.ClientException>()),
    );
    expect(attempts, 1);
  });

  test('스트리밍 요청의 본문은 복제하거나 재전송하지 않는다', () async {
    var attempts = 0;
    final client = HeaderCloseRetryClient(
      MockClient((_) async {
        attempts++;
        throw http.ClientException(closedBeforeHeaders, uri);
      }),
    );
    addTearDown(client.close);
    final request = http.StreamedRequest('GET', uri)..contentLength = 0;
    unawaited(request.sink.close());
    await expectLater(
      client.send(request),
      throwsA(isA<http.ClientException>()),
    );
    expect(attempts, 1);
  });

  for (final status in [401, 403, 500]) {
    test('HTTP $status 응답은 재전송하지 않는다', () async {
      var attempts = 0;
      final client = HeaderCloseRetryClient(
        MockClient((_) async {
          attempts++;
          return http.Response('error', status);
        }),
      );
      addTearDown(client.close);
      expect((await client.get(uri)).statusCode, status);
      expect(attempts, 1);
    });
  }

  for (final message in [
    'Connection reset by peer',
    'Connection closed while receiving data',
    'connection closed before full header was received',
  ]) {
    test('관측한 메시지와 다른 ClientException은 그대로 전파한다: $message', () async {
      var attempts = 0;
      final error = http.ClientException(message, uri);
      final client = HeaderCloseRetryClient(
        MockClient((_) async {
          attempts++;
          throw error;
        }),
      );
      addTearDown(client.close);
      await expectLater(client.get(uri), throwsA(same(error)));
      expect(attempts, 1);
    });
  }

  test('인증서 핸드셰이크 거절은 재시도하지 않는다', () async {
    var attempts = 0;
    final error = HandshakeException('certificate mismatch');
    final client = HeaderCloseRetryClient(
      MockClient((_) async {
        attempts++;
        throw error;
      }),
    );
    addTearDown(client.close);
    await expectLater(client.get(uri), throwsA(same(error)));
    expect(attempts, 1);
  });

  test('응답 헤더와 일부 본문을 받은 뒤 생긴 오류는 재전송하지 않는다', () async {
    var attempts = 0;
    Stream<List<int>> partial() async* {
      yield utf8.encode('{"success":');
      throw http.ClientException(closedBeforeHeaders, uri);
    }

    final client = HeaderCloseRetryClient(
      MockClient.streaming((_, body) async {
        attempts++;
        await body.drain<void>();
        return http.StreamedResponse(partial(), 200);
      }),
    );
    addTearDown(client.close);
    await expectLater(client.get(uri), throwsA(isA<http.ClientException>()));
    expect(attempts, 1);
  });

  test('취소 가능한 조회는 재시도에도 동일한 취소 신호를 유지한다', () async {
    final abort = Completer<void>();
    var attempts = 0;
    final client = HeaderCloseRetryClient(
      MockClient.streaming((request, body) async {
        attempts++;
        expect(request, isA<http.AbortableRequest>());
        expect(
          identical(
            (request as http.AbortableRequest).abortTrigger,
            abort.future,
          ),
          isTrue,
        );
        await body.drain<void>();
        if (attempts == 1) throw http.ClientException(closedBeforeHeaders, uri);
        return http.StreamedResponse(const Stream.empty(), 200);
      }),
    );
    addTearDown(client.close);
    expect(
      (await client.send(
        http.AbortableRequest('GET', uri, abortTrigger: abort.future),
      )).statusCode,
      200,
    );
    expect(attempts, 2);
  });

  test('DesktopClient requestJson과 기존 알림 GET 모두 동일한 인증된 복구 경로를 쓴다', () async {
    final counts = <String, int>{};
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        final route = request.url.path;
        counts[route] = (counts[route] ?? 0) + 1;
        expect(request.url.scheme, 'https');
        expect(request.url.host, 'desktop.test');
        expect(request.headers['authorization'], 'Bearer fixture-token');
        if (counts[route] == 1) {
          throw http.ClientException(closedBeforeHeaders, request.url);
        }
        return http.Response(
          jsonEncode(
            route.endsWith('notifications')
                ? {'success': true, 'notifications': []}
                : {'success': true, 'stats': {}},
          ),
          200,
        );
      }),
    );
    addTearDown(client.close);
    expect(
      (await client.requestJson(connection, 'GET', ['diff-stats']))['stats'],
      isEmpty,
    );
    expect(await client.fetchNotifications(connection), isEmpty);
    expect(counts, {
      '/api/mobile/diff-stats': 2,
      '/api/mobile/notifications': 2,
    });
  });

  test('외부 요청 timeout은 재시도마다 새로 시작하지 않는다', () async {
    final second = Completer<void>();
    var attempts = 0;
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        attempts++;
        if (attempts == 1) {
          throw http.ClientException(closedBeforeHeaders, request.url);
        }
        await second.future;
        return http.Response('{"success":true}', 200);
      }),
    );
    addTearDown(client.close);
    final request = client.requestJson(connection, 'GET', [
      'diff-stats',
    ], timeout: const Duration(milliseconds: 20));
    await expectLater(request, throwsA(isA<TimeoutException>()));
    expect(attempts, 2);
    second.complete();
  });

  test('실제 loopback 서버가 헤더 전에 socket을 닫아도 두 번째 GET은 소비된다', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    var attempts = 0;
    server.listen((request) async {
      attempts++;
      expect(request.headers.value('authorization'), 'Bearer fixture-loopback');
      if (attempts == 1) {
        final socket = await request.response.detachSocket(writeHeaders: false);
        socket.destroy();
        return;
      }
      request.response.headers.contentType = ContentType.json;
      request.response.write('{"success":true,"stats":{}}');
      await request.response.close();
    });
    final client = DesktopClient();
    addTearDown(() async {
      client.close();
      await server.close(force: true);
    });
    final loopback = DesktopConnection(
      host: '127.0.0.1',
      port: server.port,
      token: 'fixture-loopback',
    );
    final response = await client.requestJson(loopback, 'GET', ['diff-stats']);
    expect(response['stats'], isEmpty);
    expect(attempts, 2);
  });
}
