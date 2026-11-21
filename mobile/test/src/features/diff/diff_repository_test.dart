import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/features/diff/data/diff_repository.dart';

void main() {
  const connection = DesktopConnection(
    host: 'fixture',
    port: 4885,
    token: 'secret',
  );
  test('changed files retain rename paths and counts and read uses encoded task/path', () async {
    final requests = <http.Request>[];
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        requests.add(request);
        return http.Response(
          jsonEncode(
            request.url.path.endsWith('/diff')
                ? {
                    'success': true,
                    'files': [
                      {
                        'path': 'src/새 이름.dart',
                        'originalPath': 'src/old.dart',
                        'status': 'renamed',
                        'additions': 4,
                        'deletions': 2,
                      },
                    ],
                  }
                : {'success': true, 'original': 'before', 'content': 'after'},
          ),
          200,
          headers: {"content-type": "application/json; charset=utf-8"},
        );
      }),
    );
    addTearDown(client.close);
    final repository = DiffRepository(client);
    final files = await repository.fetchFiles(connection, 'task/id');
    expect(files.single.originalPath, 'src/old.dart');
    expect(files.single.additions, 4);
    final file = await repository.fetchFile(
      connection,
      'task/id',
      files.single.path,
    );
    expect(file.original, 'before');
    expect(file.content, 'after');
    expect(requests.last.url.queryParameters['path'], 'src/새 이름.dart');
    expect(requests.last.headers['authorization'], 'Bearer secret');
  });
  test('save sends the complete new file body through the authenticated PUT contract', () async {
    http.Request? saved;
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        saved = request;
        return http.Response('{"success":true}', 200);
      }),
    );
    addTearDown(client.close);
    await DiffRepository(client)
        .save(connection, 'task', 'space file.dart', '한글\nline');
    expect(saved!.method, 'PUT');
    expect(saved!.url.queryParameters['path'], 'space file.dart');
    expect(jsonDecode(saved!.body), {'content': '한글\nline'});
  });
}
