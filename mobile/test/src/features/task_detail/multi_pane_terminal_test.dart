import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:xterm/xterm.dart' as xterm;
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/task_detail/domain/mirror_pane.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/multi_pane_terminal.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/pane_terminal_view.dart';

import '../../../support/fake_desktop.dart';

const leftPane = MirrorPane(
  id: 'left',
  tabId: 'tab',
  tabName: 'tab',
  command: 'left shell',
  left: 0,
  top: 0,
  width: 50,
  height: 30,
);
const rightPane = MirrorPane(
  id: 'right',
  tabId: 'tab',
  tabName: 'tab',
  command: 'right shell',
  left: 51,
  top: 0,
  width: 49,
  height: 30,
);
const surfaces = TaskSurfaces(
  tabs: [
    MirrorTab(id: 'tab', name: 'tab', panes: [leftPane, rightPane]),
  ],
  canResize: true,
);
Widget app(FakeDesktopClient client, ValueChanged<String> select) =>
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
      child: MaterialApp(
        home: Scaffold(
          body: MultiPaneTerminal(
            taskId: 'task',
            surfaces: surfaces,
            selectedPane: leftPane,
            onSelectedPane: select,
          ),
        ),
      ),
    );
void main() {
  testWidgets(
    'tablet opens both panes, selection uses the tapped pane, and disposal closes both streams',
    (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(1024, 768);
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final client = FakeDesktopClient();
      String? selected;
      await tester.pumpWidget(app(client, (id) => selected = id));
      await tester.pump(const Duration(milliseconds: 100));
      expect(find.byType(PaneTerminalView), findsNWidgets(2));
      expect(client.openedPanes.keys.toSet(), {'left', 'right'});
      for (final stream in client.openedPanes.values) {
        stream.add('ready');
      }
      await tester.pump();
      await tester.tap(find.text('right shell · right'));
      await tester.pump();
      expect(selected, 'right');
      final rightView = tester.widget<xterm.TerminalView>(
        find.descendant(
          of: find.byWidgetPredicate(
            (widget) => widget is PaneTerminalView && widget.pane.id == 'right',
          ),
          matching: find.byType(xterm.TerminalView),
        ),
      );
      rightView.terminal.textInput('ls');
      expect(client.writtenInput['right'], contains('ls'));
      expect(client.writtenInput['left'], isNull);
      expect(client.resizedPanes['left'], isNotEmpty);
      expect(client.resizedPanes['right'], isNotEmpty);
      await tester.pumpWidget(const SizedBox());
      await tester.pump();
      expect(client.closedPanes.toSet(), {'left', 'right'});
      expect(tester.takeException(), isNull);
    },
  );
  testWidgets('phone retains the selected single-pane behavior', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(390, 844);
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final client = FakeDesktopClient();
    await tester.pumpWidget(app(client, (_) {}));
    await tester.pump(const Duration(milliseconds: 100));
    expect(find.byType(PaneTerminalView), findsOneWidget);
    expect(client.openedPanes.keys, ['left']);
    client.openedPanes['left']!.add('ready');
    await tester.pump();
    await tester.pumpWidget(const SizedBox());
    await tester.pump();
    expect(client.closedPanes, ['left']);
  });
}
