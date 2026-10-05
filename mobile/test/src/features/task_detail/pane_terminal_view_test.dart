import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
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

/// 터미널을 눌러 소프트 키보드 입력기를 붙이고, 그 입력기가 처음 IME에 건넨 버퍼를 돌려준다.
///
/// xterm은 탭마다 더블탭 판정 타이머를 건다. 그 시간을 흘려보내지 않으면 테스트가 끝날 때 타이머가 남는다
Future<String> attachSoftKeyboard(WidgetTester tester) async {
  await tester.tap(find.byType(xterm.TerminalView));
  await tester.pump(kDoubleTapTimeout);
  return tester.testTextInput.editingState?['text'] as String? ?? '';
}

/// IME가 자기 버퍼를 [previous]에서 [next]로 바꿨다고 앱에 알린다.
///
/// 엔진은 입력기가 delta 모델을 요청했으면 바뀐 부분만, 아니면 버퍼 전체를 보낸다. 입력기 구현이
/// 무엇이든 같은 IME 동작을 재현해야 수정 전후를 한 테스트로 가를 수 있으므로 설정을 보고 고른다.
/// 버퍼는 입력기가 처음 건넨 [base] 뒤에 쌓인다 — IME가 앱의 초기화 요청을 아직 반영하지 않은 상황이다.
Future<void> sendImeBuffer(
  WidgetTester tester, {
  required String base,
  required String previous,
  required String next,
}) async {
  final setClient = tester.testTextInput.log.lastWhere(
    (call) => call.method == 'TextInput.setClient',
  );
  final clientId = (setClient.arguments as List)[0] as int;
  final config = (setClient.arguments as List)[1] as Map;
  final oldText = '$base$previous';
  final newText = '$base$next';

  /// 뒤에 붙으면 끝에 끼워 넣은 것이고, 줄어들면 끝에서 지운 것이다. 이 테스트들이 쓰는 IME 동작은 이 둘뿐이다
  final isInsertion = newText.length >= oldText.length;

  final MethodCall call;
  if (config['enableDeltaModel'] == true) {
    call = MethodCall('TextInputClient.updateEditingStateWithDeltas', [
      clientId,
      {
        'deltas': [
          {
            'oldText': oldText,
            'deltaText': isInsertion ? newText.substring(oldText.length) : '',
            'deltaStart': isInsertion ? oldText.length : newText.length,
            'deltaEnd': oldText.length,
            'selectionBase': newText.length,
            'selectionExtent': newText.length,
            'selectionAffinity': 'TextAffinity.downstream',
            'selectionIsDirectional': false,
            'composingBase': -1,
            'composingExtent': -1,
          },
        ],
      },
    ]);
  } else {
    call = MethodCall('TextInputClient.updateEditingState', [
      clientId,
      TextEditingValue(
        text: newText,
        selection: TextSelection.collapsed(offset: newText.length),
      ).toJSON(),
    ]);
  }

  await tester.binding.defaultBinaryMessenger.handlePlatformMessage(
    SystemChannels.textInput.name,
    SystemChannels.textInput.codec.encodeMethodCall(call),
    (_) {},
  );
  await tester.pump();
}

void main() {
  /// pane id가 같으면 element가 재사용되므로 새 좌표는 `didUpdateWidget`으로만 들어온다.
  /// 여기서 다시 재지 않으면 상자만 커지고 격자는 옛 크기라 [FittedBox]가 잘못된 비율로 늘린다.
  testWidgets('데스크탑에서 pane 크기가 바뀌면 터미널 격자도 따라간다', (tester) async {
    final client = FakeDesktopClient();
    final store = FakeConnectionStore();
    await pumpPane(tester, _pane, client: client, store: store);
    expect(mountedTerminal(tester).viewWidth, 80);

    client.openedPanes['%19']!.add('먼저 온 출력');
    await tester.pumpAndSettle();

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
    expect(
      terminal.buffer.getText(),
      contains('먼저 온 출력'),
      reason: '크기를 다시 재도 이미 받은 화면은 남아야 한다',
    );
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

  /// Gboard는 글자를 자기 버퍼에 쌓아 두고 버퍼 전체를 기준으로 알린다. 앱이 "버퍼를 비워라"를 보내도
  /// IME가 그것을 반영하기 전에 다음 글자를 보내면, 이미 보낸 앞 글자가 다시 실려 온다.
  /// 버퍼 전체를 새 입력으로 읽으면 `t` 다음 `to`가 와서 pane에는 `tto`가 찍힌다.
  testWidgets('IME가 버퍼 초기화를 반영하기 전에 다음 글자를 보내도 앞 글자가 다시 들어가지 않는다', (
    tester,
  ) async {
    final client = FakeDesktopClient();
    await pumpPane(tester, _pane, client: client, store: FakeConnectionStore());
    final base = await attachSoftKeyboard(tester);

    await sendImeBuffer(tester, base: base, previous: '', next: 't');
    await sendImeBuffer(tester, base: base, previous: 't', next: 'to');

    expect(client.writtenInput['%19']?.join(), 'to');
  });

  /// 지우기는 이제 xterm이 아니라 이 앱의 입력기가 받는다. 글자 수만큼 백스페이스가 나가야 셸이 지운다
  testWidgets('소프트 키보드에서 지운 글자는 그 수만큼 백스페이스로 간다', (tester) async {
    final client = FakeDesktopClient();
    await pumpPane(tester, _pane, client: client, store: FakeConnectionStore());
    final base = await attachSoftKeyboard(tester);

    await sendImeBuffer(tester, base: base, previous: '', next: 'ls');
    await sendImeBuffer(tester, base: base, previous: 'ls', next: '');

    expect(client.writtenInput['%19']?.join(), 'ls\x7f\x7f');
  });

  /// 키보드의 완료 키는 IME가 동작으로 알린다. Enter로 보내지 않으면 친 명령이 실행되지 않는다
  testWidgets('소프트 키보드의 완료 키는 Enter로 간다', (tester) async {
    final client = FakeDesktopClient();
    await pumpPane(tester, _pane, client: client, store: FakeConnectionStore());
    await attachSoftKeyboard(tester);

    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pump();

    expect(client.writtenInput['%19']?.join(), '\r');
  });

  /// 상태 시트처럼 다른 화면이 위에 뜨면 터미널은 포커스를 잃는다. 키보드가 그대로 남으면
  /// 새로 뜬 화면의 아래쪽을 덮어서, 시트의 버튼을 누를 수 없게 된다
  testWidgets('터미널이 포커스를 잃으면 소프트 키보드를 내린다', (tester) async {
    await pumpPane(
      tester,
      _pane,
      client: FakeDesktopClient(),
      store: FakeConnectionStore(),
    );
    await attachSoftKeyboard(tester);
    expect(tester.testTextInput.isVisible, isTrue);

    showModalBottomSheet<void>(
      context: tester.element(find.byType(PaneTerminalView)),
      builder: (_) => const Text('시트'),
    );
    await tester.pumpAndSettle();

    expect(tester.testTextInput.isVisible, isFalse);
  });
}
