import 'dart:convert';

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
  bool canResize = false,
  bool canRespondToDeviceQueries = false,
  TextScaler textScaler = TextScaler.noScaling,
}) async {
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        desktopClientProvider.overrideWithValue(client),
        connectionStoreProvider.overrideWithValue(store),
      ],
      child: MaterialApp(
        theme: buildAppTheme(),
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(context).copyWith(textScaler: textScaler),
          child: child!,
        ),
        home: SizedBox(
          width: phoneSize.width,
          height: phoneSize.height,
          child: PaneTerminalView(
            canResize: canResize,
            canRespondToDeviceQueries: canRespondToDeviceQueries,
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

Offset terminalCellPosition(WidgetTester tester, int column, int row) {
  final render = tester
      .state<xterm.TerminalViewState>(find.byType(xterm.TerminalView))
      .renderTerminal;
  return render.localToGlobal(
    render.getOffset(xterm.CellOffset(column, row)) +
        Offset(render.cellSize.width / 2, render.cellSize.height / 2),
  );
}

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
  for (final mode in [
    (escape: '\x1b[?1006h', up: '\x1b[<64;', down: '\x1b[<65;'),
    (escape: '\x1b[?1015h', up: '\x1b[96;', down: '\x1b[97;'),
    (escape: '', up: '\x1b[M`', down: '\x1b[Ma'),
    (escape: '\x1b[?1005h', up: '\x1b[M`', down: '\x1b[Ma'),
  ]) {
    testWidgets('대체 화면 손가락 스크롤의 휠 코드 ${mode.up}', (tester) async {
      final client = FakeDesktopClient();
      await pumpPane(
        tester,
        _pane,
        client: client,
        store: FakeConnectionStore(),
        canResize: true,
      );
      client.openedPanes[_pane.id]!.add(
        '\x1b[?1049h\x1b[?1000h${mode.escape}ready',
      );
      await tester.pumpAndSettle();
      await tester.drag(find.byType(xterm.TerminalView), const Offset(0, 250));
      await tester.pumpAndSettle();
      expect((client.writtenInput[_pane.id] ?? []).join(), contains(mode.up));
      await tester.drag(find.byType(xterm.TerminalView), const Offset(0, -250));
      await tester.pumpAndSettle();
      expect((client.writtenInput[_pane.id] ?? []).join(), contains(mode.down));
      expect(tester.testTextInput.isVisible, isFalse);
    });
  }

  for (final canResize in [false, true]) {
    testWidgets('손가락으로 이전 출력과 최신 출력을 왕복한다 resize=$canResize', (tester) async {
      final client = FakeDesktopClient();
      await pumpPane(
        tester,
        _pane,
        client: client,
        store: FakeConnectionStore(),
        canResize: canResize,
      );
      client.openedPanes[_pane.id]!.add(
        List.generate(180, (i) => 'line-$i\r\n').join(),
      );
      await tester.pumpAndSettle();
      final view = tester.widget<xterm.TerminalView>(
        find.byType(xterm.TerminalView),
      );
      final scroll = view.scrollController!;
      final bottom = scroll.offset;
      expect(bottom, greaterThan(0));
      await tester.drag(find.byType(xterm.TerminalView), const Offset(0, 300));
      await tester.pumpAndSettle();
      expect(scroll.offset, lessThan(bottom));
      expect(tester.testTextInput.isVisible, isFalse);
      await tester.drag(find.byType(xterm.TerminalView), const Offset(0, -300));
      await tester.pumpAndSettle();
      expect(scroll.offset, closeTo(bottom, 1));
    });
  }

  group('OSC 52', () {
    final clipboardWrites = <String>[];
    final clipboardReads = <MethodCall>[];

    setUp(() {
      clipboardWrites.clear();
      clipboardReads.clear();
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(SystemChannels.platform, (call) async {
            if (call.method == 'Clipboard.setData') {
              clipboardWrites.add((call.arguments as Map)['text'] as String);
            } else if (call.method == 'Clipboard.getData') {
              clipboardReads.add(call);
              return {'text': '휴대폰에만 있는 내용'};
            }
            return null;
          });
    });

    tearDown(() {
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(SystemChannels.platform, null);
    });

    for (final terminator in ['\x07', '\x1b\\']) {
      testWidgets('종료 ${terminator.codeUnits} 요청은 한글과 줄바꿈을 정확히 한 번 복사한다', (
        tester,
      ) async {
        final client = FakeDesktopClient();
        await pumpPane(
          tester,
          _pane,
          client: client,
          store: FakeConnectionStore(),
        );
        const text = '  원격 Vim 복사 ✅\nsecond line\n';
        final encoded = base64Encode(utf8.encode(text));

        client.openedPanes[_pane.id]!.add(
          'before\r\n\x1b]52;c;$encoded${terminator}after',
        );
        await tester.pumpAndSettle();

        expect(clipboardWrites, [text]);
        expect(clipboardReads, isEmpty);
        expect(client.writtenInput[_pane.id] ?? [], isEmpty);
        expect(mountedTerminal(tester).buffer.lines[0].getText(), 'before');
        expect(mountedTerminal(tester).buffer.lines[1].getText(), 'after');
      });
    }

    testWidgets('모든 청크 경계에서 완결된 요청만 한 번씩 복사한다', (tester) async {
      final client = FakeDesktopClient();
      await pumpPane(
        tester,
        _pane,
        client: client,
        store: FakeConnectionStore(),
      );
      const text = '한글\nline two';
      final encoded = base64Encode(utf8.encode(text));
      final sequence = '\x1b]52;;$encoded\x1b\\';

      for (var boundary = 1; boundary < sequence.length; boundary++) {
        client.openedPanes[_pane.id]!.add(sequence.substring(0, boundary));
        await tester.pump();
        expect(clipboardWrites.length, boundary - 1, reason: 'split $boundary');
        client.openedPanes[_pane.id]!.add(sequence.substring(boundary));
        await tester.pump();
        expect(clipboardWrites.length, boundary, reason: 'split $boundary');
        expect(clipboardWrites.last, text);
      }
      expect(clipboardReads, isEmpty);
      expect(client.writtenInput[_pane.id] ?? [], isEmpty);
    });

    testWidgets('읽기·빈값·잘못된 요청은 무시하고 다음 복사와 입력을 처리한다', (tester) async {
      final client = FakeDesktopClient();
      await pumpPane(
        tester,
        _pane,
        client: client,
        store: FakeConnectionStore(),
        canRespondToDeviceQueries: true,
      );
      final encoded = base64Encode(utf8.encode('다음 복사'));
      client.openedPanes[_pane.id]!.add(
        '\x1b]52;c;?\x07'
        '\x1b]52;;?\x1b\\'
        '\x1b]52;c;\x07'
        '\x1b]52;c\x07'
        '\x1b]52;c;!not-base64!\x07'
        '\x1b]52;c;$encoded;extra\x07'
        '\x1b]99;c;$encoded\x07'
        '정상 출력',
      );
      await tester.pumpAndSettle();
      expect(clipboardWrites, isEmpty);
      expect(clipboardReads, isEmpty);
      expect(client.writtenInput[_pane.id] ?? [], isEmpty);
      expect(mountedTerminal(tester).buffer.lines[0].getText(), '정상 출력');

      client.openedPanes[_pane.id]!.add('\x1b]52;p;$encoded\x07');
      await tester.pump();
      mountedTerminal(tester).textInput('pwd');
      expect(clipboardWrites, ['다음 복사']);
      expect(client.writtenInput[_pane.id], ['pwd']);
    });

    testWidgets('데스크톱과 같은 상한까지 허용하고 초과 요청은 기존 클립보드를 유지한다', (tester) async {
      final client = FakeDesktopClient();
      await pumpPane(
        tester,
        _pane,
        client: client,
        store: FakeConnectionStore(),
      );
      final atLimit = 'A' * 1000000;
      client.openedPanes[_pane.id]!.add('\x1b]52;c;$atLimit\x07');
      await tester.pumpAndSettle();
      expect(clipboardWrites.length, 1);
      expect(clipboardWrites.single.length, 750000);

      client.openedPanes[_pane.id]!.add('\x1b]52;c;${atLimit}AAAA\x07');
      await tester.pumpAndSettle();
      expect(clipboardWrites.length, 1);
      expect(clipboardReads, isEmpty);
    });

    testWidgets('UTF-8 오류 바이트는 데스크톱처럼 대체 문자로 복사한다', (tester) async {
      final client = FakeDesktopClient();
      await pumpPane(
        tester,
        _pane,
        client: client,
        store: FakeConnectionStore(),
      );
      client.openedPanes[_pane.id]!.add('\x1b]52;c;${base64Encode([255])}\x07');
      await tester.pumpAndSettle();
      expect(clipboardWrites, ['\uFFFD']);
    });

    testWidgets('OS가 복사를 거부해도 다음 복사와 터미널 입력은 계속 동작한다', (tester) async {
      var attempts = 0;
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(SystemChannels.platform, (call) async {
            if (call.method == 'Clipboard.setData') {
              attempts++;
              if (attempts == 1) {
                throw PlatformException(code: 'clipboard_denied');
              }
              clipboardWrites.add((call.arguments as Map)['text'] as String);
            }
            return null;
          });
      final client = FakeDesktopClient();
      await pumpPane(
        tester,
        _pane,
        client: client,
        store: FakeConnectionStore(),
      );
      final encoded = base64Encode(utf8.encode('복사 재시도'));
      client.openedPanes[_pane.id]!.add('\x1b]52;c;$encoded\x07');
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);

      client.openedPanes[_pane.id]!.add(
        '\x1b]52;c;$encoded\x07still connected',
      );
      await tester.pumpAndSettle();
      mountedTerminal(tester).textInput('pwd');
      expect(clipboardWrites, ['복사 재시도']);
      expect(client.writtenInput[_pane.id], ['pwd']);
      expect(
        mountedTerminal(tester).buffer.lines[0].getText(),
        'still connected',
      );
      expect(find.byType(xterm.TerminalView), findsOneWidget);
    });
  });

  testWidgets('보조 툴바 없이 터미널이 전체 영역을 사용한다', (tester) async {
    await pumpPane(
      tester,
      _pane,
      client: FakeDesktopClient(),
      store: FakeConnectionStore(),
      canResize: true,
    );
    expect(find.text('터미널 연결 중…'), findsNothing);
    expect(find.text('출력 수신 중'), findsNothing);
    expect(find.byTooltip('붙여넣기'), findsNothing);
    expect(find.byTooltip('텍스트 선택·복사'), findsNothing);
    expect(find.byTooltip('선택한 텍스트 복사'), findsNothing);
    expect(find.text('화면 맞춤'), findsNothing);
    expect(find.text('넓게 보기'), findsNothing);
    expect(
      tester.getRect(find.byType(xterm.TerminalView)).top,
      tester.getRect(find.byType(PaneTerminalView)).top,
    );
  });

  for (final splitEveryCharacter in [false, true]) {
    testWidgets(
      'legacy 제목이 포함된 실제 ${splitEveryCharacter ? '한 글자씩 분할한' : '한 청크의'} 출력도 RIGHTEDGE를 80열에 유지한다',
      (tester) async {
        final client = FakeDesktopClient();
        await pumpPane(
          tester,
          _pane,
          client: client,
          store: FakeConnectionStore(),
        );
        final terminal = mountedTerminal(tester);
        final titles = <String>[];
        terminal.onTitleChange = titles.add;
        const prefix =
            '\r\r\n\x1b]2;printf %80s RIGHTEDGE\x1b\\\x1bkprintf\x1b\\';
        final marker = '${''.padLeft(71)}RIGHTEDGE';
        final raw = '$prefix$marker';
        final chunks = splitEveryCharacter ? raw.split('') : [raw];
        for (final chunk in chunks) {
          client.openedPanes[_pane.id]!.add(chunk);
          await tester.pump();
        }
        await tester.pumpAndSettle();
        expect(terminal.buffer.lines[1].getText(), marker);
        expect(terminal.buffer.lines[2].getText(), isEmpty);
        expect(titles, ['printf %80s RIGHTEDGE', 'printf']);
        expect(terminal.viewWidth, 80);
      },
    );
  }

  testWidgets('legacy 제목과 함께 온 일반 OSC·CSI·한글 본문은 보존한다', (tester) async {
    final client = FakeDesktopClient();
    await pumpPane(tester, _pane, client: client, store: FakeConnectionStore());
    final terminal = mountedTerminal(tester);
    final titles = <String>[];
    terminal.onTitleChange = titles.add;
    client.openedPanes[_pane.id]!.add(
      '\x1b]2;정상 제목\x1b\\'
      '\x1bklegacy 제목\x1b\\'
      '\x1b[31m본문 한글\x1b[0m\r\nkprintf '
      '\x1b[2;9H끝'
      '\x1b]0;원래 제목\x07',
    );
    await tester.pumpAndSettle();
    expect(terminal.buffer.lines[0].getText(), '본문 한글');
    expect(terminal.buffer.lines[1].getText(), 'kprintf 끝');
    expect(titles, ['정상 제목', 'legacy 제목', '원래 제목']);
  });

  testWidgets('오류 후 재연결하면 이전 스트림 끝의 ESC를 새 본문과 합치지 않는다', (tester) async {
    final client = FakeDesktopClient();
    await pumpPane(tester, _pane, client: client, store: FakeConnectionStore());
    client.openedPanes[_pane.id]!.add('\x1b');
    await tester.pump();
    client.openedPanes[_pane.id]!.addError(Exception('offline'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('다시 연결'));
    await tester.pumpAndSettle();
    client.openedPanes[_pane.id]!.add('k본문');
    await tester.pumpAndSettle();
    expect(mountedTerminal(tester).buffer.lines[0].getText(), 'k본문');
  });

  testWidgets('앱 복귀로 재연결하면 이전 스트림 끝의 ESC를 버린다', (tester) async {
    final client = FakeDesktopClient();
    await pumpPane(tester, _pane, client: client, store: FakeConnectionStore());
    client.openedPanes[_pane.id]!.add('\x1b');
    await tester.pump();
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
    await tester.pumpAndSettle();
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pumpAndSettle();
    client.openedPanes[_pane.id]!.add('k한글');
    await tester.pumpAndSettle();
    expect(mountedTerminal(tester).buffer.lines[0].getText(), 'k한글');
  });

  for (final query in [
    (name: 'DSR', data: '\x1b[6n', reply: '\x1b[0;0R'),
    (name: 'DA', data: '\x1b[c', reply: '\x1b[?1;2c'),
  ]) {
    testWidgets('tmux 미러의 ${query.name} 요청에는 응답을 추가하지 않고 사용자 입력은 보낸다', (
      tester,
    ) async {
      final client = FakeDesktopClient();
      await pumpPane(
        tester,
        _pane,
        client: client,
        store: FakeConnectionStore(),
      );
      client.openedPanes[_pane.id]!.add(query.data);
      await tester.pumpAndSettle();
      expect(client.writtenInput[_pane.id] ?? [], isEmpty);

      mountedTerminal(tester).keyInput(xterm.TerminalKey.arrowLeft);
      final base = await attachSoftKeyboard(tester);
      await sendImeBuffer(tester, base: base, previous: '', next: 'pwd');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      expect(client.writtenInput[_pane.id]!.join(), '\x1b[Dpwd\r');
    });

    testWidgets('일반 terminal의 ${query.name} 요청에는 기존 응답을 한 번 보낸다', (
      tester,
    ) async {
      final client = FakeDesktopClient();
      await pumpPane(
        tester,
        _pane,
        client: client,
        store: FakeConnectionStore(),
        canRespondToDeviceQueries: true,
      );
      client.openedPanes[_pane.id]!.add(query.data);
      await tester.pumpAndSettle();
      expect(client.writtenInput[_pane.id], [query.reply]);
      mountedTerminal(tester).textInput('user');
      expect(client.writtenInput[_pane.id], [query.reply, 'user']);
    });
  }

  testWidgets('분할 수신한 device query도 응답하지 않고 청크 사이 사용자 입력은 보낸다', (tester) async {
    final client = FakeDesktopClient();
    await pumpPane(tester, _pane, client: client, store: FakeConnectionStore());
    client.openedPanes[_pane.id]!.add('\x1b[');
    await tester.pumpAndSettle();
    expect(client.writtenInput[_pane.id] ?? [], isEmpty);
    mountedTerminal(tester).textInput('user-between');
    client.openedPanes[_pane.id]!.add('6n');
    await tester.pumpAndSettle();
    expect(client.writtenInput[_pane.id], ['user-between']);
  });

  for (final viewport in [
    (width: 320.0, scale: 1.0),
    (width: 375.0, scale: 1.0),
    (width: 411.0, scale: 1.5),
  ]) {
    testWidgets(
      '세로 ${viewport.width}폭·${viewport.scale}배에서도 셀 경계와 전송 크기가 일치한다',
      (tester) async {
        addTearDown(() => tester.binding.setSurfaceSize(null));
        await tester.binding.setSurfaceSize(Size(viewport.width, 960));
        final client = FakeDesktopClient();
        await pumpPane(
          tester,
          _pane,
          client: client,
          store: FakeConnectionStore(),
          canResize: true,
          textScaler: TextScaler.linear(viewport.scale),
        );
        final terminal = mountedTerminal(tester);
        final render = tester
            .state<xterm.TerminalViewState>(find.byType(xterm.TerminalView))
            .renderTerminal;
        expect(terminal.viewWidth, render.size.width ~/ render.cellSize.width);
        expect(
          terminal.viewHeight,
          render.size.height ~/ render.cellSize.height,
        );
        expect(client.resizedPanes[_pane.id]!.last, (
          cols: terminal.viewWidth,
          rows: terminal.viewHeight,
        ));
        expect(tester.takeException(), isNull);
      },
    );
  }

  testWidgets('툴바 없이 터치로 여러 줄을 선택해도 한글·영문·줄바꿈과 격자를 유지한다', (tester) async {
    final client = FakeDesktopClient();
    await pumpPane(
      tester,
      _pane,
      client: client,
      store: FakeConnectionStore(),
      canResize: true,
    );
    client.openedPanes[_pane.id]!.add('alpha 한글\r\nbeta next');
    await tester.pumpAndSettle();
    final view = find.byType(xterm.TerminalView);
    final controller = tester.widget<xterm.TerminalView>(view).controller;
    expect(controller, isNotNull);
    final terminal = mountedTerminal(tester);
    final originalSize = (cols: terminal.viewWidth, rows: terminal.viewHeight);
    final gesture = await tester.startGesture(
      terminalCellPosition(tester, 1, 0),
    );
    await tester.pump(kLongPressTimeout + const Duration(milliseconds: 1));
    await gesture.moveTo(terminalCellPosition(tester, 1, 1));
    await tester.pump();
    await gesture.up(timeStamp: kLongPressTimeout);
    await tester.pumpAndSettle();
    expect(tester.testTextInput.isVisible, isFalse);
    expect(terminal.buffer.getText(controller!.selection), 'alpha 한글\nbeta');
    expect((cols: terminal.viewWidth, rows: terminal.viewHeight), originalSize);
    expect(client.writtenInput[_pane.id], isNull);
  });

  testWidgets('빈 곳을 길게 눌러 선택이 없어도 키보드를 열지 않는다', (tester) async {
    await pumpPane(
      tester,
      _pane,
      client: FakeDesktopClient(),
      store: FakeConnectionStore(),
    );
    final gesture = await tester.startGesture(
      terminalCellPosition(tester, 0, 0),
    );
    await tester.pump(kLongPressTimeout + const Duration(milliseconds: 1));
    await gesture.up(timeStamp: kLongPressTimeout);
    await tester.pump(kDoubleTapTimeout);
    expect(tester.testTextInput.isVisible, isFalse);
  });

  testWidgets('선택 해제 탭은 키보드를 열지 않고 다음 짧은 탭은 입력을 시작한다', (tester) async {
    final client = FakeDesktopClient();
    await pumpPane(tester, _pane, client: client, store: FakeConnectionStore());
    client.openedPanes[_pane.id]!.add('alpha beta');
    await tester.pumpAndSettle();
    final view = tester.widget<xterm.TerminalView>(
      find.byType(xterm.TerminalView),
    );
    expect(view.controller, isNotNull);
    view.controller!.setSelection(
      view.terminal.buffer.createAnchor(0, 0),
      view.terminal.buffer.createAnchor(5, 0),
    );
    await tester.pump();
    final position = terminalCellPosition(tester, 1, 0);
    await tester.tapAt(position);
    await tester.pump(kDoubleTapTimeout);
    expect(view.controller!.selection, isNull);
    expect(tester.testTextInput.isVisible, isFalse);
    await tester.tapAt(position);
    await tester.pump(kDoubleTapTimeout);
    expect(tester.testTextInput.isVisible, isTrue);
  });

  testWidgets('손가락을 이동했다 원래 위치로 돌아와도 키보드를 열지 않는다', (tester) async {
    await pumpPane(
      tester,
      _pane,
      client: FakeDesktopClient(),
      store: FakeConnectionStore(),
    );
    final start = tester.getCenter(find.byType(xterm.TerminalView));
    final gesture = await tester.startGesture(start);
    await gesture.moveBy(const Offset(0, -60));
    await tester.pump();
    await gesture.moveTo(start);
    await gesture.up();
    await tester.pump(kDoubleTapTimeout);
    expect(tester.testTextInput.isVisible, isFalse);
  });

  testWidgets('두 손가락 접촉은 탭으로 처리하지 않는다', (tester) async {
    await pumpPane(
      tester,
      _pane,
      client: FakeDesktopClient(),
      store: FakeConnectionStore(),
    );
    final position = tester.getCenter(find.byType(xterm.TerminalView));
    final first = await tester.startGesture(position, pointer: 1);
    final second = await tester.startGesture(
      position + const Offset(30, 0),
      pointer: 2,
    );
    await second.up();
    await first.up();
    await tester.pump(kDoubleTapTimeout);
    expect(tester.testTextInput.isVisible, isFalse);
  });

  testWidgets('취소된 터치는 키보드를 열지 않고 다음 탭에 영향을 주지 않는다', (tester) async {
    await pumpPane(
      tester,
      _pane,
      client: FakeDesktopClient(),
      store: FakeConnectionStore(),
    );
    final gesture = await tester.startGesture(
      tester.getCenter(find.byType(xterm.TerminalView)),
    );
    await gesture.cancel();
    await tester.pump(kDoubleTapTimeout);
    expect(tester.testTextInput.isVisible, isFalse);
    await attachSoftKeyboard(tester);
    expect(tester.testTextInput.isVisible, isTrue);
  });

  testWidgets('폰과 가로 화면의 가용 영역에 맞춘 행·열을 서버에 보낸다', (tester) async {
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await tester.binding.setSurfaceSize(const Size(375, 812));
    final client = FakeDesktopClient();
    await pumpPane(
      tester,
      _pane,
      client: client,
      store: FakeConnectionStore(),
      canResize: true,
    );
    final terminal = mountedTerminal(tester);
    final portraitCols = terminal.viewWidth;
    final portraitRows = terminal.viewHeight;
    expect(portraitCols, lessThan(_pane.width));
    expect(portraitRows, greaterThan(_pane.height));
    expect(client.resizedPanes[_pane.id]!.last, (
      cols: portraitCols,
      rows: portraitRows,
    ));

    await tester.binding.setSurfaceSize(const Size(812, 375));
    await tester.pumpAndSettle();
    expect(terminal.viewWidth, greaterThan(portraitCols));
    expect(terminal.viewHeight, lessThan(portraitRows));
    expect(client.resizedPanes[_pane.id]!.last, (
      cols: terminal.viewWidth,
      rows: terminal.viewHeight,
    ));
    final render = tester
        .state<xterm.TerminalViewState>(find.byType(xterm.TerminalView))
        .renderTerminal;
    expect(
      terminal.viewWidth * render.cellSize.width,
      lessThanOrEqualTo(render.size.width),
    );
    expect(
      (terminal.viewWidth + 1) * render.cellSize.width,
      greaterThan(render.size.width),
    );
    expect(
      tester
          .widget<xterm.TerminalView>(find.byType(xterm.TerminalView))
          .textStyle
          .fontSize,
      14,
    );
  });

  testWidgets('화면 맞춤 모드에서 키보드를 열면 글자 크기는 유지하고 행 수만 줄인다', (tester) async {
    final client = FakeDesktopClient();
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          desktopClientProvider.overrideWithValue(client),
          connectionStoreProvider.overrideWithValue(FakeConnectionStore()),
        ],
        child: MaterialApp(
          theme: buildAppTheme(),
          home: const Scaffold(
            body: PaneTerminalView(
              taskId: 'task-1',
              pane: _pane,
              canResize: true,
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    final terminal = mountedTerminal(tester);
    final rowsBefore = terminal.viewHeight;
    final colsBefore = terminal.viewWidth;
    tester.view.viewInsets = const FakeViewPadding(bottom: 250);
    addTearDown(tester.view.resetViewInsets);
    await tester.pumpAndSettle();
    expect(terminal.viewHeight, lessThan(rowsBefore));
    expect(terminal.viewWidth, colsBefore);
    expect(client.resizedPanes[_pane.id]!.last, (
      cols: colsBefore,
      rows: terminal.viewHeight,
    ));
    expect(
      tester
          .widget<xterm.TerminalView>(find.byType(xterm.TerminalView))
          .textStyle
          .fontSize,
      14,
    );
  });

  testWidgets('앱이 배경으로 가면 연결을 닫고 돌아올 때 현재 크기를 다시 보낸다', (tester) async {
    final client = FakeDesktopClient();
    await pumpPane(
      tester,
      _pane,
      client: client,
      store: FakeConnectionStore(),
      canResize: true,
    );
    final sentBefore = client.resizedPanes[_pane.id]!.length;
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
    await tester.pumpAndSettle();
    expect(client.closedPanes, contains(_pane.id));
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pumpAndSettle();
    expect(client.resizedPanes[_pane.id]!.length, greaterThan(sentBefore));
  });

  testWidgets('네트워크 오류는 세션 종료와 구분하고 재시도할 수 있다', (tester) async {
    final client = FakeDesktopClient();
    await pumpPane(tester, _pane, client: client, store: FakeConnectionStore());
    client.openedPanes['%19']!.addError(Exception('offline'));
    await tester.pumpAndSettle();
    expect(find.textContaining('연결이 끊겼습니다'), findsOneWidget);
    await tester.tap(find.text('다시 연결'));
    await tester.pumpAndSettle();
    client.openedPanes['%19']!.add('restored');
    await tester.pumpAndSettle();
    expect(find.byType(xterm.TerminalView), findsOneWidget);
    expect(mountedTerminal(tester).buffer.getText(), contains('restored'));
  });

  testWidgets('첫 프레임이 오지 않으면 응답 지연을 알리고 재시도를 제공한다', (tester) async {
    await pumpPane(
      tester,
      _pane,
      client: FakeDesktopClient(),
      store: FakeConnectionStore(),
    );
    await tester.pump(const Duration(seconds: 15));
    expect(find.textContaining('터미널 응답이 없습니다'), findsOneWidget);
    expect(find.text('다시 연결'), findsOneWidget);
  });

  testWidgets('키보드를 열어도 글자 배율과 원본 터미널 격자는 유지된다', (tester) async {
    final client = FakeDesktopClient();
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          desktopClientProvider.overrideWithValue(client),
          connectionStoreProvider.overrideWithValue(FakeConnectionStore()),
        ],
        child: MaterialApp(
          theme: buildAppTheme(),
          home: Scaffold(
            body: PaneTerminalView(taskId: 'task-1', pane: _pane),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    final view = find.byType(xterm.TerminalView);
    final before = tester.getTopLeft(view);
    final render = tester.state<xterm.TerminalViewState>(view).renderTerminal;
    expect(render.size.width, greaterThanOrEqualTo(render.cellSize.width * 80));
    final unitBefore = tester
        .state<xterm.TerminalViewState>(view)
        .renderTerminal
        .getTransformTo(null)
        .getMaxScaleOnAxis();
    tester.view.viewInsets = const FakeViewPadding(bottom: 350);
    addTearDown(tester.view.resetViewInsets);
    await tester.pumpAndSettle();
    final unitAfter = tester
        .state<xterm.TerminalViewState>(view)
        .renderTerminal
        .getTransformTo(null)
        .getMaxScaleOnAxis();
    expect(unitBefore, 1);
    expect(unitAfter, unitBefore);
    expect(tester.getTopLeft(view).dx, before.dx);
    expect(mountedTerminal(tester).viewWidth, 80);
    expect(mountedTerminal(tester).viewHeight, 24);
    expect(tester.widget<xterm.TerminalView>(view).textStyle.fontSize, 14);
  });

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

    expect((await store.readRegistry()).connections, isEmpty);
    expect(find.textContaining('세션이 실행 중이 아닙니다'), findsNothing);
  });

  testWidgets('데스크탑 터미널 오류는 네트워크나 세션 중단으로 단정하지 않는다', (tester) async {
    final client = FakeDesktopClient()..paneCloseCodes['%19'] = 4000;
    final store = FakeConnectionStore();
    await pumpPane(tester, _pane, client: client, store: store);

    await client.openedPanes['%19']!.close();
    await tester.pumpAndSettle();

    expect(
      (await store.readRegistry()).connections,
      isNotEmpty,
      reason: '스트림 실패는 페어링을 날릴 이유가 아니다',
    );
    expect(find.textContaining('데스크탑의 터미널 스트림에 오류가 발생했습니다'), findsOneWidget);
    expect(find.textContaining('세션이 실행 중이 아닙니다'), findsNothing);
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
