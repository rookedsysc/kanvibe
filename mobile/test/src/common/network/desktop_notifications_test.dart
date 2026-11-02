import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';

const connection = DesktopConnection(host: 'localhost', port: 9736, token: 't');
const notificationJson = {
  'id': 'notice/1',
  'title': '검토 대기',
  'body': '작업을 확인해 주세요.',
  'taskId': 'task-1',
  'isRead': false,
  'createdAt': '2026-11-01T08:00:00.000Z',
};

void main() {
  test('인증 GET으로 데스크톱 알림 목록과 읽음 상태를 받는다', () async {
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        expect(request.method, 'GET');
        expect(request.url.path, '/api/mobile/notifications');
        expect(request.headers['authorization'], 'Bearer t');
        return http.Response(
          jsonEncode({
            'success': true,
            'notifications': [
              notificationJson,
              {
                ...notificationJson,
                'id': 'notice-2',
                'taskId': null,
                'isRead': true,
              },
              {'id': 1},
            ],
          }),
          200,
          headers: {'content-type': 'application/json; charset=utf-8'},
        );
      }),
    );
    addTearDown(client.close);
    final notifications = await client.fetchNotifications(connection);
    expect(notifications.map((entry) => entry.id), ['notice/1', 'notice-2']);
    expect(notifications.first.isRead, isFalse);
    expect(notifications.last.taskId, isNull);
    expect(notifications.first.createdAt, DateTime.utc(2026, 11, 1, 8));
  });

  test('개별 읽음은 ID를 인코딩한 body 없는 인증 PATCH를 사용한다', () async {
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        expect(request.method, 'PATCH');
        expect(
          request.url.toString(),
          'http://localhost:9736/api/mobile/notifications/notice%2F1/read',
        );
        expect(request.body, isEmpty);
        expect(request.headers['authorization'], 'Bearer t');
        return http.Response(
          jsonEncode({
            'success': true,
            'notification': {...notificationJson, 'isRead': true},
          }),
          200,
          headers: {'content-type': 'application/json; charset=utf-8'},
        );
      }),
    );
    addTearDown(client.close);
    final notification = await client.markNotificationRead(
      connection,
      'notice/1',
    );
    expect(notification.id, 'notice/1');
    expect(notification.isRead, isTrue);
  });

  test('전체 읽음은 body 없는 인증 POST를 사용한다', () async {
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        expect(request.method, 'POST');
        expect(request.url.path, '/api/mobile/notifications/read-all');
        expect(request.headers['authorization'], 'Bearer t');
        expect(request.body, isEmpty);
        return http.Response('{"success":true}', 200);
      }),
    );
    addTearDown(client.close);
    await client.markAllNotificationsRead(connection);
  });

  for (final status in [401, 500]) {
    for (final operation in ['list', 'read', 'read-all']) {
      test('알림 $operation의 $status HTML 응답도 상태 코드를 보존한다', () async {
        final client = DesktopClient(
          httpClient: MockClient(
            (_) async => http.Response('<html>error</html>', status),
          ),
        );
        addTearDown(client.close);
        final Future<Object?> request = switch (operation) {
          'list' => client.fetchNotifications(connection),
          'read' => client.markNotificationRead(connection, 'notice/1'),
          _ => client.markAllNotificationsRead(connection),
        };
        await expectLater(
          request,
          throwsA(
            isA<DesktopRequestException>().having(
              (error) => error.statusCode,
              'statusCode',
              status,
            ),
          ),
        );
      });
    }
  }

  test('읽음 응답이 다른 알림이면 성공으로 처리하지 않는다', () async {
    final client = DesktopClient(
      httpClient: MockClient(
        (_) async => http.Response(
          jsonEncode({
            'notification': {
              ...notificationJson,
              'id': 'other',
              'isRead': true,
            },
          }),
          200,
          headers: {'content-type': 'application/json; charset=utf-8'},
        ),
      ),
    );
    addTearDown(client.close);
    await expectLater(
      client.markNotificationRead(connection, 'notice/1'),
      throwsFormatException,
    );
  });
}
