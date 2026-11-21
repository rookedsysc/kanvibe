import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/diff/presentation/diff_screen.dart';

import '../../../support/fake_desktop.dart';

Widget app(DesktopClient client) => ProviderScope(
  overrides: [
    desktopClientProvider.overrideWithValue(client),
    connectionStoreProvider.overrideWithValue(
      FakeConnectionStore(
        initial: const SavedConnections(
          connections: [
            DesktopConnection(host: 'pc', port: 4885, token: 'paired'),
          ],
          activeId: 'pc:4885',
        ),
      ),
    ),
  ],
  child: const MaterialApp(home: DiffScreen(taskId: 'task')),
);
http.Response jsonResponse(Map<String, dynamic> body) => http.Response(
  jsonEncode({'success': true, ...body}),
  200,
  headers: {'content-type': 'application/json; charset=utf-8'},
);
void main() {
  testWidgets(
    'file review toggles Viewed, compares source and saves the entire edited text',
    (tester) async {
      String? saved;
      final client = DesktopClient(
        httpClient: MockClient((request) async {
          if (request.method == 'PUT') {
            saved = jsonDecode(request.body)['content'] as String;
            return jsonResponse({});
          }
          if (request.url.path.endsWith('/diff')) {
            return jsonResponse({
              'files': [
                {
                  'path': 'lib/main.dart',
                  'status': 'modified',
                  'additions': 2,
                  'deletions': 1,
                },
              ],
            });
          }
          return jsonResponse({'original': 'before', 'content': 'after'});
        }),
      );
      addTearDown(client.close);
      await tester.pumpWidget(app(client));
      await tester.pumpAndSettle();
      expect(find.text('main.dart'), findsOneWidget);
      await tester.tap(find.byType(Checkbox));
      await tester.pump();
      expect(tester.widget<Checkbox>(find.byType(Checkbox)).value, isTrue);
      await tester.tap(find.text('main.dart'));
      await tester.pumpAndSettle();
      expect(find.text('1  before'), findsOneWidget);
      expect(find.text('1  after'), findsOneWidget);
      await tester.tap(find.byTooltip('파일 편집'));
      await tester.pump();
      await tester.enterText(
        find.byKey(const ValueKey('diff-editor')),
        '새 본문\nline',
      );
      await tester.pump();
      await tester.tap(find.byKey(const ValueKey('diff-save')));
      await tester.pumpAndSettle();
      expect(saved, '새 본문\nline');
      expect(find.text('파일을 저장했습니다.'), findsOneWidget);
      await tester.pageBack();
      await tester.pumpAndSettle();
      expect(tester.widget<Checkbox>(find.byType(Checkbox)).value, isTrue);
    },
  );
  testWidgets(
    'load failures expose Retry and successful retry displays the empty state',
    (tester) async {
      var failed = true;
      final client = DesktopClient(
        httpClient: MockClient(
          (_) async => failed
              ? http.Response('{"success":false}', 500)
              : jsonResponse({'files': []}),
        ),
      );
      addTearDown(client.close);
      await tester.pumpWidget(app(client));
      await tester.pumpAndSettle();
      expect(find.text('변경 파일을 불러오지 못했습니다.'), findsOneWidget);
      failed = false;
      await tester.tap(find.text('다시 시도'));
      await tester.pumpAndSettle();
      expect(find.text('변경된 파일이 없습니다.'), findsOneWidget);
    },
  );
  testWidgets('leaving a dirty file requires an explicit discard', (
    tester,
  ) async {
    var writes = 0;
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        if (request.method == 'PUT') writes++;
        if (request.url.path.endsWith('/diff')) {
          return jsonResponse({
            'files': [
              {'path': 'file.txt', 'status': 'modified'},
            ],
          });
        }
        return jsonResponse({'original': 'before', 'content': 'after'});
      }),
    );
    addTearDown(client.close);
    await tester.pumpWidget(app(client));
    await tester.pumpAndSettle();
    await tester.tap(find.text('file.txt'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('파일 편집'));
    await tester.pump();
    await tester.enterText(
      find.byKey(const ValueKey('diff-editor')),
      'unsaved',
    );
    await tester.pump();
    await tester.pageBack();
    await tester.pumpAndSettle();
    expect(find.text('저장하지 않은 변경을 버릴까요?'), findsOneWidget);
    await tester.tap(find.text('계속 편집'));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('diff-editor')), findsOneWidget);
    await tester.pageBack();
    await tester.pumpAndSettle();
    await tester.tap(find.text('변경 버리기'));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('diff-editor')), findsNothing);
    expect(find.text('file.txt'), findsOneWidget);
    expect(writes, 0);
  });
}
