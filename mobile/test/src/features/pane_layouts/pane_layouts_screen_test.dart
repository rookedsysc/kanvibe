import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/board/data/board_snapshot.dart';
import 'package:kanvibe_mobile/src/features/board/domain/board_project.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/pane_layouts/presentation/pane_layouts_screen.dart';

import '../../../support/fake_desktop.dart';

void main() {
  testWidgets(
    'saves a global pane startup command and deletes the actual stored override',
    (tester) async {
      Map<String, dynamic>? saved;
      var hasLayout = false;
      final client = DesktopClient(
        httpClient: MockClient((request) async {
          if (request.method == 'PUT') {
            saved = jsonDecode(request.body) as Map<String, dynamic>;
            hasLayout = true;
          }
          if (request.method == 'DELETE') hasLayout = false;
          return http.Response(
            jsonEncode({
              'success': true,
              'layouts': hasLayout
                  ? [
                      {'id': 'stored-layout', ...saved!},
                    ]
                  : [],
            }),
            200,
            headers: {'content-type': 'application/json; charset=utf-8'},
          );
        }),
      );
      addTearDown(client.close);
      await tester.pumpWidget(
        ProviderScope(
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
            boardProvider.overrideWith(
              (ref) async => const BoardSnapshot(
                tasksByStatus: {},
                projects: [
                  BoardProject(
                    id: 'project',
                    name: 'repo',
                    defaultBranch: 'main',
                    isWorktree: false,
                  ),
                ],
              ),
            ),
          ],
          child: const MaterialApp(home: PaneLayoutsScreen()),
        ),
      );
      await tester.pumpAndSettle();
      await tester.enterText(
        find.byKey(const ValueKey('pane-command-0')),
        'printf ready',
      );
      await tester.tap(find.byKey(const ValueKey('save-pane-layout')));
      await tester.pumpAndSettle();
      expect(saved, {
        'layoutType': 'single',
        'isGlobal': true,
        'panes': [
          {'position': 0, 'command': 'printf ready'},
        ],
      });
      expect(find.text('레이아웃을 저장했습니다. 새 세션에 적용됩니다.'), findsOneWidget);
      await tester.ensureVisible(find.text('레이아웃 설정 삭제'));
      await tester.tap(find.text('레이아웃 설정 삭제'));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(FilledButton, '삭제'));
      await tester.pumpAndSettle();
      expect(hasLayout, isFalse);
      expect(find.text('레이아웃 설정 삭제'), findsNothing);
      expect(
        tester
            .widget<TextField>(find.byKey(const ValueKey('pane-command-0')))
            .controller!
            .text,
        isEmpty,
      );
    },
  );
}
