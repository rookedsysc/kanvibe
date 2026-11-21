import 'dart:async';

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

/// 가짜 시계가 필요해 위젯 테스트 바인딩에서 돈다. 바인딩이 실제 소켓을 막으므로 TLS를 여는 테스트와 파일을 나눈다
void main() {
  for (final wait in [
    (seconds: 20, arrives: true),
    (seconds: 31, arrives: false),
  ]) {
    testWidgets('꺼져 있던 세션을 띄우느라 ${wait.seconds}초 걸린 탭 목록 응답', (tester) async {
      final client = DesktopClient(
        httpClient: MockClient((_) async {
          await Future<void>.delayed(Duration(seconds: wait.seconds));
          return http.Response(
            '{"surfaces":{"tabs":[{"id":"@10","name":"kanvibe","panes":[]}]}}',
            200,
          );
        }),
      );
      TaskSurfaces? surfaces;
      Object? failure;
      unawaited(
        client
            .fetchSurfaces(_connection, 'task-1')
            .then<void>(
              (value) => surfaces = value,
              onError: (Object error) => failure = error,
            ),
      );

      await tester.pump(Duration(seconds: wait.seconds));
      await tester.pump();

      expect(surfaces?.tabs.length, wait.arrives ? 1 : null);
      expect(failure, wait.arrives ? isNull : isA<TimeoutException>());
    });
  }
}
