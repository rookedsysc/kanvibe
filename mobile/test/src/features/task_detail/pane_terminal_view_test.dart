import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/constants/app_theme.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/task_detail/domain/mirror_pane.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/pane_terminal_view.dart';
import 'package:xterm/xterm.dart' as xterm;

import '../../../support/fake_desktop.dart';

const _pane = MirrorPane(
  id: '%19',
  tabId: '@10',
  tabName: 'kanvibe',
  command: 'claude',
  left: 0,
  top: 0,
  width: 80,
  height: 24,
);

/// pane 하나만 띄운다. 화면 조립은 `task_detail_screen_test`가 보고, 여기서는 이 위젯의 수명만 본다
Future<void> pumpPane(
  WidgetTester tester,
  MirrorPane pane, {
  required FakeDesktopClient client,
  required FakeConnectionStore store,
}) async {
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        desktopClientProvider.overrideWithValue(client),
        connectionStoreProvider.overrideWithValue(store),
      ],
      child: MaterialApp(
        theme: buildAppTheme(),
        home: SizedBox(
          width: phoneSize.width,
          height: phoneSize.height,
          child: PaneTerminalView(
            taskId: 'task-1',
            pane: pane,
            key: ValueKey(pane.id),
          ),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

xterm.Terminal mountedTerminal(WidgetTester tester) =>
    tester.widget<xterm.TerminalView>(find.byType(xterm.TerminalView)).terminal;

void main() {
  /// pane id가 같으면 element가 재사용되므로 새 좌표는 `didUpdateWidget`으로만 들어온다.
  /// 여기서 다시 재지 않으면 상자만 커지고 격자는 옛 크기라 [FittedBox]가 잘못된 비율로 늘린다.
  testWidgets('데스크탑에서 pane 크기가 바뀌면 터미널 격자도 따라간다', (tester) async {
    final client = FakeDesktopClient();
    final store = FakeConnectionStore();
    await pumpPane(tester, _pane, client: client, store: store);
    expect(mountedTerminal(tester).viewWidth, 80);

    await pumpPane(
      tester,
      const MirrorPane(
        id: '%19',
        tabId: '@10',
        tabName: 'kanvibe',
        command: 'claude',
        left: 0,
        top: 0,
        width: 120,
        height: 40,
      ),
      client: client,
      store: store,
    );

    final terminal = mountedTerminal(tester);
    expect(terminal.viewWidth, 120);
    expect(terminal.viewHeight, 40);
  });

  /// 서버는 "이 기기는 더 이상 연결되어 있지 않다"를 4001로 따로 알린다.
  /// 그것을 세션 문제로 접으면 사용자는 데스크탑에서 세션을 여닫으며 원인을 찾고, 다시 페어링할 길은 화면에 없다.
  testWidgets('해제 코드로 끊기면 저장된 연결을 비워 다시 페어링할 수 있게 한다', (tester) async {
    final client = FakeDesktopClient()..paneCloseCodes['%19'] = 4001;
    final store = FakeConnectionStore();
    await pumpPane(tester, _pane, client: client, store: store);

    await client.openedPanes['%19']!.close();
    await tester.pumpAndSettle();

    expect(await store.read(), isNull);
    expect(find.textContaining('세션이 실행 중이 아닙니다'), findsNothing);
  });

  testWidgets('그 밖의 이유로 끊기면 연결은 두고 사유만 보여 준다', (tester) async {
    final client = FakeDesktopClient()..paneCloseCodes['%19'] = 4000;
    final store = FakeConnectionStore();
    await pumpPane(tester, _pane, client: client, store: store);

    await client.openedPanes['%19']!.close();
    await tester.pumpAndSettle();

    expect(await store.read(), isNotNull, reason: '스트림 실패는 페어링을 날릴 이유가 아니다');
    expect(find.textContaining('세션이 실행 중이 아닙니다'), findsOneWidget);
  });
}
