import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/features/pane_layouts/data/pane_layout_repository.dart';
import 'package:kanvibe_mobile/src/features/pane_layouts/domain/pane_layout.dart';

void main() {
  const connection = DesktopConnection(host: 'pc', port: 4885, token: 'paired');
  test('global and project layouts use the real typed contract and ordered pane commands', () async {
    final requests = <http.Request>[];
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        requests.add(request);
        return http.Response(
          jsonEncode({
            'success': true,
            'layouts': [
              {
                'id': 'layout',
                'isGlobal': false,
                'projectId': 'project',
                'layoutType': 'vertical_2',
                'panes': [
                  {'position': 1, 'command': 'right'},
                  {'position': 0, 'command': 'left'},
                ],
              },
            ],
          }),
          200,
        );
      }),
    );
    addTearDown(client.close);
    final repository = PaneLayoutRepository(client);
    final layouts = await repository.fetch(connection);
    expect(layouts.single.commands, ['left', 'right']);
    await repository.save(connection, PaneLayoutKind.vertical2, [
      'left',
      'right',
    ], 'project');
    expect(jsonDecode(requests.last.body), {
      'isGlobal': false,
      'projectId': 'project',
      'layoutType': 'vertical_2',
      'panes': [
        {'position': 0, 'command': 'left'},
        {'position': 1, 'command': 'right'},
      ],
    });
    await repository.save(connection, PaneLayoutKind.single, [''], null);
    expect(
      (jsonDecode(requests.last.body) as Map).containsKey('projectId'),
      isFalse,
    );
    expect(jsonDecode(requests.last.body)['isGlobal'], isTrue);
    await repository.delete(connection, 'layout');
    expect(requests.last.method, 'DELETE');
    expect(requests.last.url.path, '/api/mobile/pane-layouts/layout');
    expect(
      requests.every(
        (request) => request.headers['authorization'] == 'Bearer paired',
      ),
      isTrue,
    );
  });
}
