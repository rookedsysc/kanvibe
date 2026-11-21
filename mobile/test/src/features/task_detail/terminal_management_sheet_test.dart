import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/task_detail/domain/mirror_pane.dart';
import 'package:kanvibe_mobile/src/features/task_detail/data/terminal_management_repository.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/terminal_management_sheet.dart';

import '../../../support/fake_desktop.dart';

void main() {
  tearDown(
    () => TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(ClipboardImageReader.channel, null),
  );
  testWidgets(
    'tab rename and accessible order controls send the selected tab mutation',
    (tester) async {
      final calls =
          <({String method, List<String> path, Map<String, dynamic>? body})>[];
      final client =
          FakeDesktopClient(
              surfaces: const TaskSurfaces(
                tabs: [
                  MirrorTab(id: '@1', name: 'first', panes: []),
                  MirrorTab(id: '@2', name: 'second', panes: []),
                ],
              ),
            )
            ..onJsonRequest = (method, segments, query, body) async {
              calls.add((method: method, path: segments, body: body));
              return {'success': true};
            };
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
          ],
          child: const MaterialApp(
            home: Scaffold(
              body: TerminalManagementSheet(
                taskId: 'task',
                selectedPaneId: '%1',
                isRemote: true,
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text('first'));
      await tester.pumpAndSettle();
      await tester.enterText(
        find.byKey(const ValueKey('terminal-tab-name')),
        'renamed',
      );
      await tester.tap(find.widgetWithText(FilledButton, '변경'));
      await tester.pumpAndSettle();
      expect(calls.last.method, 'PATCH');
      expect(calls.last.path, ['tasks', 'task', 'tabs', '@1']);
      expect(calls.last.body, {'name': 'renamed'});
      await tester.tap(find.byTooltip('탭 뒤로 이동').first);
      await tester.pumpAndSettle();
      expect(calls.last.method, 'POST');
      expect(calls.last.path, ['tasks', 'task', 'tabs', '@1', 'move']);
      expect(calls.last.body, {'index': 1});
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(
            ClipboardImageReader.channel,
            (_) async => null,
          );
      final before = calls.length;
      await tester.tap(find.byKey(const ValueKey('paste-clipboard-image')));
      await tester.pumpAndSettle();
      expect(find.text('클립보드에 이미지가 없습니다.'), findsOneWidget);
      expect(calls.length, before);
    },
  );
}
