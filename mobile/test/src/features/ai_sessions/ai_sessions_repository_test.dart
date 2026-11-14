import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/features/ai_sessions/data/ai_sessions_repository.dart';

const pc = DesktopConnection(host: 'fixture', port: 9746, token: 'paired');
void main() {
  test('실행 목록은 상태·하위 작업·window ID를 읽고 task ID를 인코딩한다', () async {
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        expect(request.url.pathSegments, [
          'api',
          'mobile',
          'tasks',
          'task/1',
          'live-ai-sessions',
        ]);
        return http.Response(
          jsonEncode({
            'success': true,
            'liveSessions': {
              'taskId': 'task/1',
              'isRemote': true,
              'sessions': [
                {
                  'provider': 'claude',
                  'sessionId': 's1',
                  'currentTask': 'Fix task',
                  'state': 'running',
                  'lastActiveAt': '2026-11-13T00:00:00Z',
                  'runningSubtasks': [
                    {'id': 'child', 'name': 'Explore', 'lastActiveAt': null},
                  ],
                  'terminalWindow': {'windowId': '@2', 'windowName': 'Agent'},
                },
              ],
            },
          }),
          200,
        );
      }),
    );
    addTearDown(client.close);
    final snapshot = await AiSessionsRepository(client)
        .fetchSessions(pc, 'task/1');
    expect(snapshot.sessions.single.terminalWindowId, '@2');
    expect(snapshot.sessions.single.runningSubtasks.single.name, 'Explore');
    expect(snapshot.isRemote, isTrue);
  });
  test('호출 그래프는 provider·session을 query로 보내고 중첩 구조와 시간을 읽는다', () async {
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        expect(request.url.queryParameters, {
          'provider': 'codex',
          'sessionId': 's1',
        });
        return http.Response(
          jsonEncode({
            'success': true,
            'graph': {
              'provider': 'codex',
              'sessionId': 's1',
              'startedAt': '2026-11-13T00:00:00Z',
              'readAt': '2026-11-13T00:01:00Z',
              'roots': [
                {
                  'id': 'a',
                  'agentType': 'worker',
                  'task': 'Root',
                  'skill': null,
                  'startedAt': '2026-11-13T00:00:00Z',
                  'endedAt': null,
                  'children': [
                    {
                      'id': 'b',
                      'agentType': 'Explore',
                      'task': 'Child',
                      'skill': 'search',
                      'startedAt': '2026-11-13T00:00:10Z',
                      'endedAt': '2026-11-13T00:00:30Z',
                      'children': [],
                    },
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
    final graph = await AiSessionsRepository(client)
        .fetchGraph(pc, 'task-1', 'codex', 's1');
    expect(graph!.roots.single.children.single.skill, 'search');
    expect(
      graph.roots.single.children.single.endedAt,
      DateTime.utc(2026, 11, 13, 0, 0, 30),
    );
  });
  test('없는 그래프는 정상 빈 결과로 구분한다', () async {
    final client = DesktopClient(
      httpClient: MockClient(
        (_) async => http.Response('{"success":true,"graph":null}', 200),
      ),
    );
    addTearDown(client.close);
    expect(
      await AiSessionsRepository(client)
          .fetchGraph(pc, 'task', 'claude', 'session'),
      isNull,
    );
  });
}
