import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/features/task_detail/data/task_details_api.dart';

const connection = DesktopConnection(host: 'pc', port: 4885, token: 'paired');
http.Response json(Map<String, dynamic> body, [int status = 200]) =>
    http.Response(
      jsonEncode({'success': status == 200, ...body}),
      status,
      headers: {'content-type': 'application/json; charset=utf-8'},
    );
const task = {
  'id': 'task',
  'title': '작업',
  'status': 'progress',
  'description': '설명',
  'priority': 'high',
  'projectId': 'project',
  'project': {'id': 'project', 'name': '원격 프로젝트', 'color': '#123456'},
};

void main() {
  test('details save sends only editable fields then re-reads authoritative enriched task', () async {
    final calls = <http.Request>[];
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        calls.add(request);
        return json({'task': task});
      }),
    );
    addTearDown(client.close);
    final updated = await TaskDetailsApi(
      client,
      connection,
    ).update('task', description: '변경\n전체 본문', priority: null);
    expect(calls.map((r) => r.method), ['PATCH', 'GET']);
    expect(calls.first.url.path, '/api/mobile/tasks/task/details');
    expect(jsonDecode(calls.first.body), {
      'description': '변경\n전체 본문',
      'priority': null,
    });
    expect(calls.last.url.path, '/api/mobile/tasks/task');
    expect(
      calls.every((r) => r.headers['authorization'] == 'Bearer paired'),
      isTrue,
    );
    expect(updated.id, 'task');
    expect(updated.projectName, '원격 프로젝트');
    expect(updated.projectColor, '#123456');
  });

  test(
    'failed detail save never performs an authoritative success re-read',
    () async {
      final methods = <String>[];
      final client = DesktopClient(
        httpClient: MockClient((r) async {
          methods.add(r.method);
          return json({}, 409);
        }),
      );
      addTearDown(client.close);
      await expectLater(
        TaskDetailsApi(client, connection).update('task', description: 'text'),
        throwsA(isA<DesktopRequestException>()),
      );
      expect(methods, ['PATCH']);
    },
  );

  test('project color changes the selected project without rewriting task metadata', () async {
    late http.Request call;
    final client = DesktopClient(
      httpClient: MockClient((r) async {
        call = r;
        return json({'color': '#abcdef'});
      }),
    );
    addTearDown(client.close);
    await TaskDetailsApi(client, connection).setColor('project', '#abcdef');
    expect(call.method, 'PATCH');
    expect(call.url.path, '/api/mobile/projects/project/color');
    expect(jsonDecode(call.body), {'color': '#abcdef'});
  });

  test('related and base branch links retain project scope and encoded branch query', () async {
    final calls = <http.Request>[];
    final client = DesktopClient(
      httpClient: MockClient((r) async {
        calls.add(r);
        return r.url.path.endsWith('/tasks')
            ? json({
                'tasks': [task],
              })
            : json({'task': task});
      }),
    );
    addTearDown(client.close);
    final api = TaskDetailsApi(client, connection);
    expect((await api.related('project')).single.id, 'task');
    expect((await api.baseTask('project', 'feat/한글 branch'))?.id, 'task');
    expect(calls.first.url.path, '/api/mobile/projects/project/tasks');
    expect(calls.last.url.path, '/api/mobile/projects/project/task');
    expect(calls.last.url.queryParameters, {'branch': 'feat/한글 branch'});
  });

  test('missing base task and missing pull request remain empty instead of inventing a link', () async {
    final calls = <http.Request>[];
    final client = DesktopClient(
      httpClient: MockClient((r) async {
        calls.add(r);
        return json({'task': null, 'prUrl': null});
      }),
    );
    addTearDown(client.close);
    final api = TaskDetailsApi(client, connection);
    expect(await api.baseTask('project', 'main'), isNull);
    expect(await api.findPullRequest('task'), isNull);
    expect(calls.last.method, 'POST');
    expect(calls.last.url.path, '/api/mobile/tasks/task/pr/refresh');
  });
}
