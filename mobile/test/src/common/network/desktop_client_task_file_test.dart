import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/features/chat_history/domain/task_file.dart';

const connection = DesktopConnection(host: 'a', port: 9736, token: 'token-a');

DesktopClient clientAnswering(int statusCode, Object body) => DesktopClient(
  httpClient: MockClient(
    (_) async => http.Response(
      jsonEncode(body),
      statusCode,
      headers: {'content-type': 'application/json; charset=utf-8'},
    ),
  ),
);

void main() {
  test('파일 요청은 인증과 URL 인코딩한 경로를 보내고 본문을 돌려준다', () async {
    late http.Request sent;
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        sent = request;
        return http.Response(
          jsonEncode({
            'success': true,
            'file': {'path': 'docs/내 계획.md', 'content': '# 계획'},
          }),
          200,
          headers: {'content-type': 'application/json; charset=utf-8'},
        );
      }),
    );
    addTearDown(client.close);

    final file = await client.fetchTaskFile(
      connection,
      'task/1',
      '/repo/docs/내 계획.md?v=1&x',
    );

    expect(sent.method, 'GET');
    expect(sent.headers['authorization'], 'Bearer token-a');
    expect(
      sent.url.toString(),
      'http://a:9736/api/mobile/tasks/task%2F1/workspace-file'
      '?path=%2Frepo%2Fdocs%2F%EB%82%B4+%EA%B3%84%ED%9A%8D.md%3Fv%3D1%26x',
    );
    expect(sent.url.queryParameters, {'path': '/repo/docs/내 계획.md?v=1&x'});
    expect(file.path, 'docs/내 계획.md');
    expect(file.content, '# 계획');
  });

  for (final (statusCode, failure) in [
    (400, TaskFileFailure.outsideTaskFolder),
    (404, TaskFileFailure.notFound),
    (413, TaskFileFailure.tooLarge),
  ]) {
    test('파일 요청의 $statusCode 응답은 ${failure.name} 사유로 알린다', () async {
      final client = clientAnswering(statusCode, {'success': false});
      addTearDown(client.close);

      await expectLater(
        client.fetchTaskFile(connection, 'task-1', '/repo/a.md'),
        throwsA(
          isA<TaskFileException>().having(
            (error) => error.failure,
            'failure',
            failure,
          ),
        ),
      );
    });
  }

  test('파일 요청의 그 밖의 실패 응답은 상태 코드를 담은 요청 실패로 알린다', () async {
    final client = clientAnswering(500, {'error': '읽기 실패'});
    addTearDown(client.close);

    await expectLater(
      client.fetchTaskFile(connection, 'task-1', '/repo/a.md'),
      throwsA(
        isA<DesktopRequestException>().having(
          (error) => error.statusCode,
          'statusCode',
          500,
        ),
      ),
    );
  });
}
