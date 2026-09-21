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
