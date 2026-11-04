import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/constants/app_theme.dart';
import 'package:kanvibe_mobile/src/features/task_create/domain/session_type.dart';
import 'package:kanvibe_mobile/src/features/task_detail/data/terminal_shortcut_store.dart';
import 'package:kanvibe_mobile/src/features/task_detail/domain/terminal_shortcut.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/terminal_shortcut_bar.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/terminal_shortcut_combo_editor.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/terminal_shortcut_settings_sheet.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/terminal_shortcut_panel.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/terminal_soft_keyboard.dart';
import 'package:xterm/xterm.dart' as xterm;

const _copy = TerminalShortcutChord(
  key: 'character',
  character: 'c',
  ctrl: true,
);

Future<void> _saveCombos(
  List<TerminalShortcut> combos, {
  List<String>? ids,
}) async {
  final store = TerminalShortcutStore();
  final profiles = await store.read();
  profiles[SessionType.tmux] = TerminalShortcutProfile(
    ids: ids ?? combos.map((combo) => combo.id).toList(),
    combos: combos,
  );
  await store.write(profiles);
}

Future<void> _pumpBar(
  WidgetTester tester, {
  SessionType session = SessionType.tmux,
  ValueChanged<TerminalShortcut>? onShortcut,
  bool isBusy = false,
}) async {
  await tester.pumpWidget(
    ProviderScope(
      child: MaterialApp(
        theme: buildAppTheme(),
        home: Scaffold(
          body: Align(
            alignment: Alignment.bottomCenter,
            child: TerminalShortcutBar(
              sessionType: session,
              onShortcut: onShortcut ?? (_) {},
              isBusy: isBusy,
            ),
          ),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

Future<void> _pumpKeyboard(WidgetTester tester, xterm.Terminal terminal) async {
  final controller = xterm.TerminalController();
  addTearDown(controller.dispose);
  await tester.pumpWidget(
    ProviderScope(
      child: MaterialApp(
        theme: buildAppTheme(),
        home: Scaffold(
          body: TerminalSoftKeyboard(
            terminal: terminal,
            controller: controller,
            sessionType: SessionType.tmux,
            child: const ColoredBox(
              key: ValueKey('terminal-surface'),
              color: Colors.black,
            ),
          ),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  setUp(() => FlutterSecureStorage.setMockInitialValues({}));

  testWidgets('combo-only bar excludes default unregistered keys', (
    tester,
  ) async {
    await _pumpBar(tester);
    expect(find.widgetWithText(TextButton, 'Esc'), findsNothing);
    expect(find.widgetWithText(TextButton, 'Tab'), findsNothing);
    expect(find.text('콤보 추가'), findsOneWidget);
    expect(
      tester.getSize(find.byType(TerminalShortcutBar)).height,
      lessThanOrEqualTo(40),
    );
  });

  testWidgets('only selected saved chords appear in the shortcut bar', (
    tester,
  ) async {
    await _saveCombos(
      [
        TerminalShortcut.combo('combo-copy', 'Copy', _copy),
        TerminalShortcut.combo(
          'combo-hide',
          'Hidden',
          const TerminalShortcutChord(key: 'escape'),
        ),
      ],
      ids: ['tab', 'combo-copy'],
    );
    await _pumpBar(tester);
    expect(find.widgetWithText(TextButton, 'Copy'), findsOneWidget);
    expect(find.widgetWithText(TextButton, 'Hidden'), findsNothing);
    expect(find.widgetWithText(TextButton, 'Tab'), findsNothing);
  });

  testWidgets('shortcut bar collapses immediately and can be restored', (
    tester,
  ) async {
    await _pumpKeyboard(tester, xterm.Terminal());
    await tester.tap(find.byKey(const ValueKey('terminal-surface')));
    await tester.pump();
    expect(tester.testTextInput.isVisible, isTrue);
    final before = tester
        .getSize(find.byKey(const ValueKey('terminal-surface')))
        .height;
    await tester.tap(find.byTooltip('단축키 바 접기'));
    await tester.pump();
    expect(find.byType(TerminalShortcutBar), findsNothing);
    expect(tester.testTextInput.isVisible, isTrue);
    expect(
      tester.getSize(find.byKey(const ValueKey('terminal-surface'))).height,
      greaterThan(before),
    );
    await tester.tap(find.byTooltip('단축키 바 펼치기'));
    await tester.pump();
    expect(find.byType(TerminalShortcutBar), findsOneWidget);
    expect(tester.testTextInput.isVisible, isTrue);
    expect(
      (await TerminalShortcutStore().read())[SessionType.tmux]!.enabled,
      isTrue,
    );
  });

  test(
    'legacy single chords migrate but sequences never execute or overwrite v1',
    () async {
      final legacy = jsonEncode({
        'tmux': {
          'enabled': false,
          'ids': ['escape', 'combo-copy', 'combo-sequence'],
          'combos': [
            {
              'id': 'combo-copy',
              'label': 'Copy',
              'strokes': [
                {'key': 'character', 'character': 'c', 'ctrl': true},
              ],
            },
            {
              'id': 'combo-sequence',
              'label': 'Sequence',
              'strokes': [
                {'key': 'up'},
                {'key': 'enter'},
              ],
            },
            {
              'id': 'combo-hidden',
              'label': 'Hidden',
              'strokes': [
                {'key': 'escape'},
              ],
            },
          ],
        },
      });
      FlutterSecureStorage.setMockInitialValues({
        'kanvibe_terminal_shortcuts_v1': legacy,
      });
      final store = TerminalShortcutStore();
      final profiles = await store.read();
      expect(profiles[SessionType.tmux]!.enabled, isFalse);
      expect(profiles[SessionType.tmux]!.ids, ['combo-copy']);
      expect(profiles[SessionType.tmux]!.combos.map((combo) => combo.id), [
        'combo-copy',
        'combo-hidden',
      ]);
      expect(profiles[SessionType.tmux]!.combos.first.chord!.label, 'Ctrl+c');
      await store.write(profiles);
      const storage = FlutterSecureStorage();
      expect(await storage.read(key: 'kanvibe_terminal_shortcuts_v1'), legacy);
      final current = jsonDecode(
        (await storage.read(key: 'kanvibe_terminal_shortcuts_v2'))!,
      );
      expect(current['tmux']['combos'].first.containsKey('strokes'), isFalse);
      expect(current['tmux']['combos'].first['chord']['ctrl'], isTrue);
    },
  );

  test(
    'shift and hidden chords round trip independently per session',
    () async {
      final combo = TerminalShortcut.combo(
        'combo-copy',
        'Copy',
        const TerminalShortcutChord(
          key: 'character',
          character: 'c',
          ctrl: true,
          alt: true,
          shift: true,
        ),
      );
      await _saveCombos([combo], ids: []);
      final profiles = await TerminalShortcutStore().read();
      final restored = profiles[SessionType.tmux]!.combos.single;
      expect(restored.description, 'Ctrl+Alt+Shift+c');
      expect(restored.chord!.shift, isTrue);
      expect(profiles[SessionType.tmux]!.ids, isEmpty);
      expect(profiles[SessionType.zellij]!.combos, isEmpty);
    },
  );

  test('invalid current storage does not resurrect legacy shortcuts', () async {
    FlutterSecureStorage.setMockInitialValues({
      'kanvibe_terminal_shortcuts_v2': '{broken',
      'kanvibe_terminal_shortcuts_v1': jsonEncode({
        'tmux': {
          'ids': ['combo-copy'],
          'combos': [
            {
              'id': 'combo-copy',
              'label': 'Copy',
              'strokes': [
                {'key': 'escape'},
              ],
            },
          ],
        },
      }),
    });
    final profile = (await TerminalShortcutStore().read())[SessionType.tmux]!;
    expect(profile.combos, isEmpty);
    expect(profile.ids, isEmpty);
  });

  test(
    'malformed or sequential current definitions cannot reach execution',
    () async {
      final good = {
        'id': 'combo-good',
        'label': 'Good',
        'chord': {'key': 'escape'},
      };
      FlutterSecureStorage.setMockInitialValues({
        'kanvibe_terminal_shortcuts_v2': jsonEncode({
          'tmux': {
            'ids': [
              'tab',
              'combo-good',
              'combo-bad',
              'combo-seq',
              'combo-type',
              'combo-action',
            ],
            'combos': [
              good,
              good,
              {
                'id': 'combo-bad',
                'label': 'Bad',
                'chord': {'key': 'character', 'character': 'two'},
              },
              {
                'id': 'combo-seq',
                'label': 'Bad',
                'strokes': [
                  {'key': 'up'},
                  {'key': 'enter'},
                ],
              },
              {
                'id': 'combo-type',
                'label': 'Bad',
                'chord': {'key': 'escape', 'shift': 'yes'},
              },
              {
                'id': 'combo-action',
                'label': 'Bad',
                'chord': {'key': 'split-right'},
              },
            ],
          },
        }),
      });
      final profile = (await TerminalShortcutStore().read())[SessionType.tmux]!;
      expect(profile.ids, ['combo-good']);
      expect(profile.combos.map((combo) => combo.id), ['combo-good']);
    },
  );

  test('special picker excludes ordinary mobile keys and validates modified characters', () {
    final keys = terminalComboKeys().map((key) => key.id);
    expect(
      keys,
      containsAll(['escape', 'tab', 'up', 'f12', 'insert', 'delete']),
    );
    expect(keys, isNot(contains('enter')));
    expect(keys, isNot(contains('backspace')));
    expect(keys, isNot(contains('character')));
    expect(
      const TerminalShortcutChord(
        key: 'character',
        character: '한',
        ctrl: true,
      ).isValid,
      isFalse,
    );
    expect(
      const TerminalShortcutChord(key: 'character', character: '').isValid,
      isFalse,
    );
    expect(
      const TerminalShortcutChord(
        key: 'character',
        character: '2',
        ctrl: true,
        shift: true,
      ).isValid,
      isTrue,
    );
    expect(
      const TerminalShortcutChord(
        key: 'character',
        character: 'ß',
        shift: true,
      ).characterWithShift,
      'ß',
    );
  });

  final cases = <(String, TerminalShortcutChord, String)>[
    (
      'Ctrl+Alt+c',
      const TerminalShortcutChord(
        key: 'character',
        character: 'c',
        ctrl: true,
        alt: true,
      ),
      '\x1b\x03',
    ),
    (
      'Ctrl+Shift+c',
      const TerminalShortcutChord(
        key: 'character',
        character: 'c',
        ctrl: true,
        shift: true,
      ),
      '\x03',
    ),
    (
      'Shift+c',
      const TerminalShortcutChord(
        key: 'character',
        character: 'c',
        shift: true,
      ),
      'C',
    ),
    (
      'Shift+1',
      const TerminalShortcutChord(
        key: 'character',
        character: '1',
        shift: true,
      ),
      '!',
    ),
    (
      'Ctrl+Shift+2',
      const TerminalShortcutChord(
        key: 'character',
        character: '2',
        ctrl: true,
        shift: true,
      ),
      '\x00',
    ),
    (
      'Shift+Tab',
      const TerminalShortcutChord(key: 'tab', shift: true),
      '\x1b[Z',
    ),
    (
      'Shift+Up',
      const TerminalShortcutChord(key: 'up', shift: true),
      '\x1b[1;2A',
    ),
    (
      'Ctrl+Alt+Shift+Right',
      const TerminalShortcutChord(
        key: 'right',
        ctrl: true,
        alt: true,
        shift: true,
      ),
      '\x1b[1;8C',
    ),
    (
      'Shift+PgUp',
      const TerminalShortcutChord(key: 'pageUp', shift: true),
      '\x1b[5;2~',
    ),
    (
      'Ctrl+F5',
      const TerminalShortcutChord(key: 'f5', ctrl: true),
      '\x1b[15;5~',
    ),
    ('Esc', const TerminalShortcutChord(key: 'escape'), '\x1b'),
    ('Enter', const TerminalShortcutChord(key: 'enter'), '\r'),
  ];
  for (final (name, chord, expected) in cases) {
    testWidgets('$name sends one simultaneous chord per press', (tester) async {
      await _saveCombos([TerminalShortcut.combo('combo-wire', 'Wire', chord)]);
      final output = <String>[];
      await _pumpKeyboard(tester, xterm.Terminal()..onOutput = output.add);
      await tester.tap(find.widgetWithText(TextButton, 'Wire'));
      await tester.tap(find.widgetWithText(TextButton, 'Wire'));
      expect(output, [expected, expected]);
    });
  }

  testWidgets(
    'plain cursor chord follows cursor mode and consumes old modifiers',
    (tester) async {
      await _saveCombos([
        TerminalShortcut.combo(
          'combo-up',
          'Up',
          const TerminalShortcutChord(key: 'up'),
        ),
      ]);
      final output = <String>[];
      final terminal = xterm.Terminal()..onOutput = output.add;
      terminal.write('\x1b[?1h');
      await _pumpKeyboard(tester, terminal);
      await tester.tap(find.byTooltip('빠른 단축키'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('키'));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('panel-key-ctrl')));
      await tester.tap(find.widgetWithText(TextButton, 'Up'));
      expect(output, ['\x1bOA']);
    },
  );

  for (final session in SessionType.values) {
    testWidgets(
      '${session.name} quick panel routes only supported session actions',
      (tester) async {
        final pressed = <TerminalShortcut>[];
        await _pumpBar(tester, session: session, onShortcut: pressed.add);
        await tester.tap(find.byTooltip('빠른 단축키'));
        await tester.pumpAndSettle();
        expect(
          tester.getSize(find.byType(TerminalShortcutPanel)).height,
          lessThanOrEqualTo(140),
        );
        if (session == SessionType.terminal) {
          expect(find.text('패널 제어'), findsNothing);
          await tester.tap(find.byKey(const ValueKey('panel-key-escape')));
          expect(pressed.single.id, 'escape');
        } else {
          await tester.tap(find.byKey(const ValueKey('panel-key-split-right')));
          expect(pressed.single.action, 'split-right');
          expect(
            find.byKey(const ValueKey('panel-key-copy-mode')),
            session == SessionType.tmux ? findsOneWidget : findsNothing,
          );
        }
      },
    );
  }

  testWidgets('busy panel blocks session controls but permits raw keys', (
    tester,
  ) async {
    final pressed = <TerminalShortcut>[];
    await _pumpBar(tester, onShortcut: pressed.add, isBusy: true);
    await tester.tap(find.byTooltip('빠른 단축키'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('panel-key-split-right')));
    expect(pressed, isEmpty);
    await tester.tap(find.text('키'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('panel-key-escape')));
    expect(pressed.single.id, 'escape');
  });

  testWidgets('chord editor offers Shift without sequential steps', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        theme: buildAppTheme(),
        home: const Scaffold(body: TerminalShortcutComboEditor()),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('Shift'), findsOneWidget);
    expect(find.textContaining('키 단계 추가'), findsNothing);
    await tester.tap(find.text('콤보 저장'));
    await tester.pump();
    expect(find.textContaining('이름과 조합할 키'), findsOneWidget);
    await tester.enterText(find.widgetWithText(TextField, '콤보 이름'), 'Copy');
    await tester.enterText(find.widgetWithText(TextField, '입력 키'), 'c');
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilterChip, 'Ctrl'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilterChip, 'Shift'));
    await tester.pumpAndSettle();
    await tester.pump();
    expect(find.text('Ctrl+Shift+c'), findsOneWidget);
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pump();
    expect(find.text('Ctrl+Shift+c'), findsOneWidget);
  });

  testWidgets(
    'editor captures Enter and Backspace from keyboard instead of picker',
    (tester) async {
      await tester.pumpWidget(
        MaterialApp(
          theme: buildAppTheme(),
          home: const Scaffold(body: TerminalShortcutComboEditor()),
        ),
      );
      await tester.enterText(find.widgetWithText(TextField, '입력 키'), 'c');
      await tester.enterText(find.widgetWithText(TextField, '입력 키'), '');
      await tester.pump();
      expect(find.text('Backspace'), findsOneWidget);
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pump();
      expect(find.text('Enter'), findsOneWidget);
      await tester.pumpAndSettle();
      await tester.ensureVisible(find.text('특수 키 선택'));
      await tester.tap(find.text('특수 키 선택'));
      await tester.pump();
      expect(find.text('Enter'), findsNothing);
      expect(find.text('Backspace'), findsNothing);
      expect(
        find.byKey(const ValueKey('combo-special-escape')),
        findsOneWidget,
      );
    },
  );

  testWidgets('draft cancel, Apply, edit and delete preserve a single chord', (
    tester,
  ) async {
    await _pumpBar(tester);
    Future<void> addCombo(String name) async {
      await tester.tap(find.textContaining('콤보 추가 ('));
      await tester.pumpAndSettle();
      await tester.enterText(find.widgetWithText(TextField, '콤보 이름'), name);
      await tester.enterText(find.widgetWithText(TextField, '입력 키'), 'c');
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(FilterChip, 'Ctrl'));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(FilterChip, 'Shift'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('콤보 저장'));
      await tester.pumpAndSettle();
    }

    await tester.tap(find.byTooltip('tmux 단축키 설정'));
    await tester.pumpAndSettle();
    await addCombo('Cancelled');
    await tester.tap(find.byTooltip('닫기'));
    await tester.pumpAndSettle();
    expect(
      (await TerminalShortcutStore().read())[SessionType.tmux]!.combos,
      isEmpty,
    );
    await tester.tap(find.byTooltip('tmux 단축키 설정'));
    await tester.pumpAndSettle();
    await addCombo('Saved');
    await tester.tap(find.text('적용'));
    await tester.pumpAndSettle();
    expect(
      (await TerminalShortcutStore().read())[SessionType.tmux]!
          .combos
          .single
          .chord!
          .label,
      'Ctrl+Shift+c',
    );

    Future<void> openMenu(String label) async {
      await tester.tap(find.byTooltip('tmux 단축키 설정'));
      await tester.pumpAndSettle();
      await tester.scrollUntilVisible(
        find.byTooltip('$label 콤보 동작'),
        200,
        scrollable: find
            .descendant(
              of: find.byType(TerminalShortcutSettingsSheet),
              matching: find.byType(Scrollable),
            )
            .first,
      );
      await tester.tap(find.byTooltip('$label 콤보 동작'));
      await tester.pumpAndSettle();
    }

    await openMenu('Saved');
    await tester.tap(find.text('콤보 편집'));
    await tester.pumpAndSettle();
    await tester.enterText(find.widgetWithText(TextField, '콤보 이름'), 'Edited');
    await tester.enterText(find.widgetWithText(TextField, '입력 키'), 'r');
    await tester.tap(find.text('콤보 저장'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('적용'));
    await tester.pumpAndSettle();
    final edited =
        (await TerminalShortcutStore().read())[SessionType.tmux]!.combos.single;
    expect(edited.chord!.label, 'Ctrl+Shift+r');
    await openMenu('Edited');
    await tester.tap(find.text('콤보 삭제'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('적용'));
    await tester.pumpAndSettle();
    expect(
      (await TerminalShortcutStore().read())[SessionType.tmux]!.combos,
      isEmpty,
    );
    expect(find.text('콤보 추가'), findsOneWidget);
  });

  testWidgets('quick panel stays below 104dp and scrolls to the last action', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(390, 844);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final pressed = <TerminalShortcut>[];
    await _pumpBar(tester, onShortcut: pressed.add);
    await tester.tap(find.byTooltip('빠른 단축키'));
    await tester.pumpAndSettle();
    expect(
      tester.getSize(find.byType(TerminalShortcutPanel)).height,
      lessThanOrEqualTo(104),
    );
    expect(find.text('pane을 좌우로 분할'), findsNothing);
    final last = find.byKey(const ValueKey('panel-key-copy-mode'));
    await tester.ensureVisible(last);
    await tester.pumpAndSettle();
    await tester.tap(last);
    expect(pressed.single.action, 'copy-mode');
    expect(tester.takeException(), isNull);
  });

  for (final screen in ['panel', 'editor', 'settings']) {
    testWidgets('$screen fits small viewport with large text and IME', (
      tester,
    ) async {
      tester.view.physicalSize = const Size(320, 640);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final controller = xterm.TerminalController();
      addTearDown(controller.dispose);
      final child = switch (screen) {
        'panel' => TerminalSoftKeyboard(
          terminal: xterm.Terminal(),
          controller: controller,
          sessionType: SessionType.tmux,
          child: const SizedBox(),
        ),
        'editor' => const TerminalShortcutComboEditor(),
        _ => const TerminalShortcutSettingsSheet(sessionType: SessionType.tmux),
      };
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            theme: buildAppTheme(),
            home: MediaQuery(
              data: MediaQueryData(
                size: const Size(320, 640),
                viewInsets: EdgeInsets.only(
                  bottom: screen == 'settings' ? 0 : 250,
                ),
                textScaler: const TextScaler.linear(2),
              ),
              child: Scaffold(
                resizeToAvoidBottomInset: screen == 'panel',
                body: child,
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      if (screen == 'panel') {
        await tester.tap(find.byTooltip('빠른 단축키'));
        await tester.pumpAndSettle();
        expect(
          tester.getSize(find.byType(TerminalShortcutPanel)).height,
          lessThanOrEqualTo(140),
        );
      }
      expect(tester.takeException(), isNull);
    });
  }
}
