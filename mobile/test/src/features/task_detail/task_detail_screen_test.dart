import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/features/task_detail/domain/mirror_pane.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/task_detail_screen.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/pane_terminal_view.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/window_pane_composition.dart';
import 'package:xterm/xterm.dart' as xterm;

import '../../../support/fake_desktop.dart';

/// 데스크탑 스크린샷과 같은 모양. window 하나가 좌우 두 pane으로 갈려 있고, 두 번째 window가 하나 더 있다
const _twoPaneWindow = MirrorTab(
  id: '@10',
  name: 'kanvibe',
  panes: [
    MirrorPane(
      id: '%19',
      tabId: '@10',
      tabName: 'kanvibe',
      command: 'claude',
      left: 0,
      top: 0,
      width: 50,
      height: 30,
    ),
    MirrorPane(
      id: '%20',
      tabId: '@10',
      tabName: 'kanvibe',
      command: 'zsh',
      left: 51,
      top: 0,
      width: 49,
      height: 30,
    ),
  ],
);

const _secondWindow = MirrorTab(
  id: '@11',
  name: 'second',
  panes: [
    MirrorPane(
      id: '%21',
      tabId: '@11',
      tabName: 'second',
      command: 'bash',
      left: 0,
      top: 0,
      width: 100,
      height: 30,
    ),
  ],
);

const _surfaces = TaskSurfaces(tabs: [_twoPaneWindow, _secondWindow]);

Future<FakeDesktopClient> pumpDetail(
  WidgetTester tester, {
  required Size size,
  TaskSurfaces? surfaces = _surfaces,
  Object? surfacesError,
}) async {
  await tester.binding.setSurfaceSize(size);
  final client = FakeDesktopClient(
    surfaces: surfaces,
    surfacesError: surfacesError,
  );

  await tester.pumpWidget(
    wrapWithApp(
      const TaskDetailScreen(taskId: 'task-1', taskTitle: '모바일 클라이언트'),
      client: client,
      size: size,
    ),
  );
  await tester.pumpAndSettle();
  return client;
}

/// pane이 실제로 키를 받는 자리. 입력이 어느 소켓으로 나가는지는 여기서만 확인할 수 있다
xterm.Terminal terminalIn(WidgetTester tester, String paneId) => tester
    .widget<xterm.TerminalView>(
      find.descendant(
        of: find.byKey(ValueKey(paneId)),
        matching: find.byType(xterm.TerminalView),
      ),
    )
    .terminal;

void main() {
  testWidgets('폰에서는 모든 window의 pane이 각각 탭이 된다', (tester) async {
    await pumpDetail(tester, size: phoneSize);

    /// window 두 개에 걸친 pane 세 개가 전부 탭으로 나와야 한다
    expect(find.text('claude'), findsOneWidget);
    expect(find.text('zsh'), findsOneWidget);
    expect(find.text('bash'), findsOneWidget);
  });

  testWidgets('폰에서는 고른 pane 하나만 화면에 보인다', (tester) async {
    await pumpDetail(tester, size: phoneSize);

    expect(find.byType(PaneTerminalView), findsOneWidget);
    expect(find.byType(WindowPaneComposition), findsNothing);
  });

  testWidgets('폰에서 다른 탭을 고르면 그 pane으로 바뀐다', (tester) async {
    final client = await pumpDetail(tester, size: phoneSize);
    expect(client.openedPanes.keys, ['%19']);

    /// pane이 많으면 탭 바가 옆으로 넘치므로 사용자처럼 밀어서 꺼낸 뒤 누른다
    await tester.ensureVisible(find.text('bash'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('bash'));
    await tester.pumpAndSettle();

    expect(client.openedPanes.keys, contains('%21'));
    expect(client.closedPanes, contains('%19'), reason: '떠난 pane의 구독은 닫혀야 한다');
  });

  testWidgets('태블릿에서는 window가 탭이 되고 pane이 함께 보인다', (tester) async {
    await pumpDetail(tester, size: tabletSize);

    expect(find.text('kanvibe'), findsOneWidget);
    expect(find.text('second'), findsOneWidget);
    expect(find.byType(WindowPaneComposition), findsOneWidget);
    expect(
      find.byType(PaneTerminalView),
      findsNWidgets(2),
      reason: '고른 window의 pane 둘이 함께 보인다',
    );
  });

  testWidgets('태블릿은 pane을 데스크탑과 같은 좌우 배치로 놓는다', (tester) async {
    await pumpDetail(tester, size: tabletSize);

    final panes = tester
        .widgetList<PaneTerminalView>(find.byType(PaneTerminalView))
        .toList();
    final left = tester.getTopLeft(find.byWidget(panes[0]));
    final right = tester.getTopLeft(find.byWidget(panes[1]));

    expect(right.dx, greaterThan(left.dx), reason: '두 번째 pane이 오른쪽에 온다');
    expect(right.dy, equals(left.dy), reason: '같은 줄에 놓인다');
  });

  testWidgets('태블릿은 처음에 첫 pane이 입력을 받는다', (tester) async {
    await pumpDetail(tester, size: tabletSize);

    final panes = tester
        .widgetList<PaneTerminalView>(find.byType(PaneTerminalView))
        .toList();

    expect(panes[0].isInteractive, isTrue);
    expect(panes[1].isInteractive, isFalse, reason: '입력은 한 pane만 받는다');
  });

  testWidgets('태블릿에서 다른 pane을 누르면 구독은 그대로 두고 입력만 옮겨 간다', (tester) async {
    final client = await pumpDetail(tester, size: tabletSize);

    /// 읽기 전용 pane은 포인터를 막아 두었으므로 탭은 그 자리를 감싼 프레임이 받는다
    await tester.tapAt(tester.getCenter(find.byKey(const ValueKey('%20'))));
    await tester.pumpAndSettle();

    expect(
      client.closedPanes,
      isEmpty,
      reason: '입력 대상만 바뀐 것이라 두 pane의 구독과 스크롤백은 살아 있어야 한다',
    );

    terminalIn(tester, '%20').textInput('ls');
    terminalIn(tester, '%19').textInput('여기로는 가면 안 된다');

    expect(client.writtenInput['%20'], contains('ls'));
    expect(
      client.writtenInput['%19'],
      isNull,
      reason: '입력을 넘겨준 pane은 더 이상 키를 데스크탑으로 보내지 않는다',
    );
  });

  testWidgets('화면을 떠나면 pane 구독이 닫힌다', (tester) async {
    final client = await pumpDetail(tester, size: phoneSize);
    expect(client.closedPanes, isEmpty);

    await tester.pumpWidget(
      wrapWithApp(const SizedBox.shrink(), client: client, size: phoneSize),
    );
    await tester.pumpAndSettle();

    expect(client.closedPanes, contains('%19'));
  });

  testWidgets('세션이 꺼져 있으면 빈 터미널 대신 사유와 재시도를 보여 준다', (tester) async {
    await pumpDetail(
      tester,
      size: phoneSize,
      surfaces: null,
      surfacesError: const SurfaceUnavailableException(
        SurfaceUnavailableReason.sessionNotRunning,
      ),
    );

    expect(find.textContaining('세션이 실행 중이 아닙니다'), findsOneWidget);
    expect(find.text('다시 시도'), findsOneWidget);
    expect(find.byType(PaneTerminalView), findsNothing);
  });

  testWidgets('zellij 버전이 낮으면 필요한 버전을 알려 준다', (tester) async {
    await pumpDetail(
      tester,
      size: phoneSize,
      surfaces: null,
      surfacesError: const SurfaceUnavailableException(
        SurfaceUnavailableReason.zellijTooOld,
      ),
    );

    expect(find.textContaining('zellij 0.44 이상'), findsOneWidget);
  });

  testWidgets('pane이 하나도 없으면 세션이 꺼진 것으로 안내한다', (tester) async {
    await pumpDetail(
      tester,
      size: phoneSize,
      surfaces: const TaskSurfaces(tabs: []),
    );

    expect(find.textContaining('세션이 실행 중이 아닙니다'), findsOneWidget);
  });
}
