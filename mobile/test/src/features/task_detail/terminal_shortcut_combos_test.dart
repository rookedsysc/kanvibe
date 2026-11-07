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
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/terminal_shortcut_settings_page.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/terminal_soft_keyboard.dart';
import 'package:xterm/xterm.dart' as xterm;

const _copy = TerminalShortcutChord(keys: ['c'], ctrl: true);

Future<void> _saveCombos(
  List<TerminalShortcut> combos, {
  List<String>? ids,
  SessionType session = SessionType.tmux,
}) async {
  final store = TerminalShortcutStore();
  final profiles = await store.read();
  profiles[session] = TerminalShortcutProfile(
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

  testWidgets('unchecked defaults stay off the bar until chosen', (
    tester,
  ) async {
    await _pumpBar(tester);
    expect(find.widgetWithText(TextButton, 'Esc'), findsNothing);
    expect(find.widgetWithText(TextButton, 'Tab'), findsNothing);
    expect(find.text('단축키 추가'), findsOneWidget);
    expect(
      tester.getSize(find.byType(TerminalShortcutBar)).height,
      lessThanOrEqualTo(40),
    );
  });

  testWidgets('session label is plain text without a quick panel', (
    tester,
  ) async {
    await _pumpBar(tester);
    await tester.tap(find.text('tmux'));
    await tester.pumpAndSettle();
    expect(find.byTooltip('빠른 단축키'), findsNothing);
    expect(find.byType(ChoiceChip), findsNothing);
    expect(find.text('좌우 분할'), findsNothing);
  });

  testWidgets('checked defaults and chords share the bar in saved order', (
    tester,
  ) async {
    await _saveCombos(
      [
        TerminalShortcut.combo('combo-copy', 'Copy', _copy),
        TerminalShortcut.combo(
          'combo-hide',
          'Hidden',
          const TerminalShortcutChord(keys: ['escape']),
        ),
      ],
      ids: ['combo-copy', 'tab', 'split-right'],
    );
    final pressed = <TerminalShortcut>[];
    await _pumpBar(tester, onShortcut: pressed.add);
    final copy = find.widgetWithText(TextButton, 'Copy');
    final tab = find.widgetWithText(TextButton, 'Tab');
    final split = find.widgetWithText(TextButton, '좌우 분할');
    expect(find.widgetWithText(TextButton, 'Hidden'), findsNothing);
    expect(tester.getCenter(copy).dx, lessThan(tester.getCenter(tab).dx));
    expect(tester.getCenter(tab).dx, lessThan(tester.getCenter(split).dx));
    await tester.tap(tab);
    await tester.tap(split);
    expect(pressed.map((shortcut) => shortcut.id), ['tab', 'split-right']);
    expect(pressed.last.action, 'split-right');
  });

  testWidgets('busy bar blocks session actions but keeps keys', (tester) async {
    await _saveCombos([], ids: ['split-right', 'escape']);
    final pressed = <TerminalShortcut>[];
    await _pumpBar(tester, onShortcut: pressed.add, isBusy: true);
    await tester.tap(find.widgetWithText(TextButton, '좌우 분할'));
    await tester.tap(find.widgetWithText(TextButton, 'Esc'));
    expect(pressed.map((shortcut) => shortcut.id), ['escape']);
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
        (await storage.read(key: 'kanvibe_terminal_shortcuts_v3'))!,
      );
      expect(current['tmux']['combos'].first.containsKey('strokes'), isFalse);
      expect(current['tmux']['combos'].first['chord']['ctrl'], isTrue);
      expect(current['tmux']['combos'].first['chord']['keys'], ['c']);
    },
  );

  test('v2 single-key chords migrate to v3 key lists without loss', () async {
    final singleKeyChords = jsonEncode({
      'zellij': {
        'enabled': true,
        'ids': ['combo-pane', 'combo-f5'],
        'combos': [
          {
            'id': 'combo-pane',
            'label': 'Pane',
            'chord': {'key': 'character', 'character': 'p', 'ctrl': true},
          },
          {
            'id': 'combo-f5',
            'label': 'F5',
            'chord': {'key': 'f5', 'character': '', 'alt': true},
          },
        ],
      },
    });
    FlutterSecureStorage.setMockInitialValues({
      'kanvibe_terminal_shortcuts_v2': singleKeyChords,
    });
    final store = TerminalShortcutStore();
    final profiles = await store.read();
    final combos = profiles[SessionType.zellij]!.combos;
    expect(combos.map((combo) => combo.chord!.label), ['Ctrl+p', 'Alt+F5']);
    expect(combos.map((combo) => combo.chord!.keys), [
      ['p'],
      ['f5'],
    ]);
    await store.write(profiles);
    const storage = FlutterSecureStorage();
    expect(
      await storage.read(key: 'kanvibe_terminal_shortcuts_v2'),
      singleKeyChords,
    );
    final current = jsonDecode(
      (await storage.read(key: 'kanvibe_terminal_shortcuts_v3'))!,
    );
    expect(
      current['zellij']['combos'].first['chord'].containsKey('key'),
      isFalse,
    );
    expect(current['zellij']['ids'], ['combo-pane', 'combo-f5']);
  });

  test(
    'shift and hidden chords round trip independently per session',
    () async {
      final combo = TerminalShortcut.combo(
        'combo-copy',
        'Copy',
        const TerminalShortcutChord(
          keys: ['up', 'c', 'd'],
          ctrl: true,
          alt: true,
          shift: true,
        ),
      );
      await _saveCombos([combo], ids: []);
      final profiles = await TerminalShortcutStore().read();
      final restored = profiles[SessionType.tmux]!.combos.single;
      expect(restored.description, 'Ctrl+Alt+Shift+↑+c+d');
      expect(restored.chord!.keys, ['up', 'c', 'd']);
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

  test('malformed v3 key lists cannot reach execution', () async {
    Map<String, Object> combo(String id, Object keys) => {
      'id': id,
      'label': id,
      'chord': {'keys': keys, 'ctrl': true},
    };
    FlutterSecureStorage.setMockInitialValues({
      'kanvibe_terminal_shortcuts_v3': jsonEncode({
        'tmux': {
          'ids': ['combo-good'],
          'combos': [
            combo('combo-good', ['a', 'b']),
            combo('combo-empty', <String>[]),
            combo('combo-dup', ['a', 'a']),
            combo('combo-type', [1]),
            combo('combo-action', ['split-right']),
            combo('combo-hangul', ['한']),
            combo('combo-many', ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i']),
          ],
        },
      }),
    });
    final profile = (await TerminalShortcutStore().read())[SessionType.tmux]!;
    expect(profile.combos.map((combo) => combo.id), ['combo-good']);
    expect(profile.combos.single.chord!.label, 'Ctrl+a+b');
  });

  test('special picker offers keys the text field cannot type and validates characters', () {
    final keys = terminalComboKeys().map((key) => key.id);
    expect(
      keys,
      containsAll([
        'escape',
        'tab',
        'up',
        'f12',
        'insert',
        'delete',
        'enter',
        'backspace',
      ]),
    );
    expect(keys, isNot(contains('ctrl')));
    expect(keys, isNot(contains('alt')));
    expect(keys, isNot(contains('interrupt')));
    expect(
      const TerminalShortcutChord(keys: ['한'], ctrl: true).isValid,
      isFalse,
    );
    expect(const TerminalShortcutChord(keys: []).isValid, isFalse);
    expect(
      const TerminalShortcutChord(keys: ['2'], ctrl: true, shift: true).isValid,
      isTrue,
    );
    expect(
      const TerminalShortcutChord(keys: [], shift: true).shiftedCharacter('ß'),
      'ß',
    );
  });

  final cases = <(String, TerminalShortcutChord, String)>[
    (
      'Ctrl+Alt+c',
      const TerminalShortcutChord(keys: ['c'], ctrl: true, alt: true),
      '\x1b\x03',
    ),
    (
      'Ctrl+Shift+c',
      const TerminalShortcutChord(keys: ['c'], ctrl: true, shift: true),
      '\x03',
    ),
    ('Shift+c', const TerminalShortcutChord(keys: ['c'], shift: true), 'C'),
    ('Shift+1', const TerminalShortcutChord(keys: ['1'], shift: true), '!'),
    (
      'Ctrl+Shift+2',
      const TerminalShortcutChord(keys: ['2'], ctrl: true, shift: true),
      '\x00',
    ),
    (
      'Shift+Tab',
      const TerminalShortcutChord(keys: ['tab'], shift: true),
      '\x1b[Z',
    ),
    (
      'Shift+Up',
      const TerminalShortcutChord(keys: ['up'], shift: true),
      '\x1b[1;2A',
    ),
    (
      'Ctrl+Alt+Shift+Right',
      const TerminalShortcutChord(
        keys: ['right'],
        ctrl: true,
        alt: true,
        shift: true,
      ),
      '\x1b[1;8C',
    ),
    (
      'Shift+PgUp',
      const TerminalShortcutChord(keys: ['pageUp'], shift: true),
      '\x1b[5;2~',
    ),
    (
      'Ctrl+F5',
      const TerminalShortcutChord(keys: ['f5'], ctrl: true),
      '\x1b[15;5~',
    ),
    (
      'Alt+F5',
      const TerminalShortcutChord(keys: ['f5'], alt: true),
      '\x1b[15;3~',
    ),
    ('Esc', const TerminalShortcutChord(keys: ['escape']), '\x1b'),
    ('Enter', const TerminalShortcutChord(keys: ['enter']), '\r'),
    ('F5', const TerminalShortcutChord(keys: ['f5']), '\x1b[15~'),
    (
      'Ctrl+b (tmux prefix)',
      const TerminalShortcutChord(keys: ['b'], ctrl: true),
      '\x02',
    ),
    (
      'Ctrl+a+b',
      const TerminalShortcutChord(keys: ['a', 'b'], ctrl: true),
      '\x01\x02',
    ),
    (
      'Ctrl+Shift+Up+Right',
      const TerminalShortcutChord(
        keys: ['up', 'right'],
        ctrl: true,
        shift: true,
      ),
      '\x1b[1;6A\x1b[1;6C',
    ),
    ('a+b', const TerminalShortcutChord(keys: ['a', 'b']), 'ab'),
  ];
  for (final (name, chord, expected) in cases) {
    testWidgets('$name sends every key with the held modifiers per press', (
      tester,
    ) async {
      await _saveCombos([TerminalShortcut.combo('combo-wire', 'Wire', chord)]);
      final output = <String>[];
      await _pumpKeyboard(tester, xterm.Terminal()..onOutput = output.add);
      await tester.tap(find.widgetWithText(TextButton, 'Wire'));
      await tester.tap(find.widgetWithText(TextButton, 'Wire'));
      expect(output.join(), '$expected$expected');
    });
  }

  testWidgets(
    'plain cursor chord follows cursor mode and consumes old modifiers',
    (tester) async {
      await _saveCombos(
        [
          TerminalShortcut.combo(
            'combo-up',
            'Up',
            const TerminalShortcutChord(keys: ['up']),
          ),
        ],
        ids: ['ctrl', 'combo-up'],
      );
      final output = <String>[];
      final terminal = xterm.Terminal()..onOutput = output.add;
      terminal.write('\x1b[?1h');
      await _pumpKeyboard(tester, terminal);
      await tester.tap(find.bySemanticsIdentifier('shortcut-ctrl'));
      await tester.tap(find.widgetWithText(TextButton, 'Up'));
      expect(output, ['\x1bOA']);
    },
  );

  Future<List<TerminalShortcut?>> openEditor(
    WidgetTester tester, {
    TerminalShortcut? combo,
  }) async {
    final results = <TerminalShortcut?>[];
    await tester.pumpWidget(
      MaterialApp(
        theme: buildAppTheme(),
        home: Builder(
          builder: (context) => TextButton(
            onPressed: () async => results.add(
              await Navigator.of(context).push<TerminalShortcut>(
                MaterialPageRoute(
                  builder: (_) => TerminalShortcutComboEditor(combo: combo),
                ),
              ),
            ),
            child: const Text('open'),
          ),
        ),
      ),
    );
    await tester.tap(find.text('open'));
    await tester.pumpAndSettle();
    return results;
  }

  Future<void> typeKeys(WidgetTester tester, String text) async {
    final field = find.widgetWithText(TextField, '입력 키');
    await tester.dragUntilVisible(
      field,
      find.byType(ListView),
      const Offset(0, -200),
    );
    await tester.enterText(field, text);
    await tester.pumpAndSettle();
  }

  Future<void> tapKey(WidgetTester tester, String id) async {
    final chip = find.byKey(ValueKey(id));
    await tester.ensureVisible(chip);
    await tester.pumpAndSettle();
    await tester.tap(chip);
    await tester.pumpAndSettle();
  }

  testWidgets(
    'editor combines several special keys and typed keys into one chord',
    (tester) async {
      final results = await openEditor(tester);
      await tester.enterText(find.widgetWithText(TextField, '콤보 이름'), 'Mix');
      await tapKey(tester, 'combo-modifier-ctrl');
      await tapKey(tester, 'combo-modifier-shift');
      await tapKey(tester, 'combo-special-up');
      await tapKey(tester, 'combo-special-f5');
      await typeKeys(tester, 'abb');
      await tester.drag(find.byType(ListView), const Offset(0, 2000));
      await tester.pumpAndSettle();
      expect(find.bySemanticsLabel('Ctrl+Shift+↑+F5+a+b'), findsOneWidget);
      await tapKey(tester, 'combo-special-f5');
      await tester.tap(find.text('저장'));
      await tester.pumpAndSettle();
      final chord = results.single!.chord!;
      expect(chord.keys, ['up', 'a', 'b']);
      expect(chord.ctrl && chord.shift && !chord.alt, isTrue);
      expect(results.single!.label, 'Mix');
    },
  );

  testWidgets('editor accepts a single key and explains each invalid draft', (
    tester,
  ) async {
    final results = await openEditor(tester);
    await tester.tap(find.text('저장'));
    await tester.pump();
    expect(find.text('콤보 이름을 입력해 주세요.'), findsOneWidget);
    await tester.enterText(find.widgetWithText(TextField, '콤보 이름'), 'One');
    await tester.tap(find.text('저장'));
    await tester.pump();
    expect(find.text('함께 누를 키를 하나 이상 골라 주세요.'), findsOneWidget);
    await tapKey(tester, 'combo-modifier-ctrl');
    await typeKeys(tester, '한');
    await tester.tap(find.text('저장'));
    await tester.pump();
    expect(find.textContaining('영문이나 기호만'), findsOneWidget);
    expect(results, isEmpty);
    await typeKeys(tester, '');
    await tapKey(tester, 'combo-modifier-ctrl');
    await tapKey(tester, 'combo-special-enter');
    await tester.tap(find.text('저장'));
    await tester.pumpAndSettle();
    expect(results.single!.chord!.label, 'Enter');
  });

  testWidgets('editing restores the saved modifiers, special and typed keys', (
    tester,
  ) async {
    final results = await openEditor(
      tester,
      combo: TerminalShortcut.combo(
        'combo-edit',
        'Edit',
        const TerminalShortcutChord(keys: ['escape', 'x'], alt: true),
      ),
    );
    expect(find.text('콤보 편집'), findsOneWidget);
    expect(find.bySemanticsLabel('Alt+Esc+x'), findsOneWidget);
    await tester.tap(find.text('저장'));
    await tester.pumpAndSettle();
    expect(results.single!.id, 'combo-edit');
    expect(results.single!.chord!.keys, ['escape', 'x']);
  });

  Finder settingsScrollable() => find
      .descendant(
        of: find.byType(TerminalShortcutSettingsPage),
        matching: find.byType(Scrollable),
      )
      .first;

  Future<void> tapInSettings(WidgetTester tester, Finder target) async {
    await tester.scrollUntilVisible(
      target,
      200,
      scrollable: settingsScrollable(),
    );
    await tester.pumpAndSettle();
    await tester.tap(target);
    await tester.pumpAndSettle();
  }

  testWidgets('draft cancel, Apply, edit and delete preserve the chord', (
    tester,
  ) async {
    await _pumpBar(tester);
    final addButton = find.descendant(
      of: find.byType(TerminalShortcutSettingsPage),
      matching: find.widgetWithText(TextButton, '콤보 추가'),
    );
    Future<void> addCombo(String name) async {
      await tapInSettings(tester, addButton);
      await tester.enterText(find.widgetWithText(TextField, '콤보 이름'), name);
      await tapKey(tester, 'combo-modifier-ctrl');
      await typeKeys(tester, 'b');
      await tester.tap(find.text('저장'));
      await tester.pumpAndSettle();
    }

    await tester.tap(find.byTooltip('tmux 단축키 설정'));
    await tester.pumpAndSettle();
    expect(find.text('tmux 단축키'), findsOneWidget);
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
    expect(find.byType(TerminalShortcutSettingsPage), findsNothing);
    expect(find.widgetWithText(TextButton, 'Saved'), findsOneWidget);
    expect(
      (await TerminalShortcutStore().read())[SessionType.tmux]!
          .combos
          .single
          .chord!
          .label,
      'Ctrl+b',
    );

    Future<void> openMenu(String label) async {
      await tester.tap(find.byTooltip('tmux 단축키 설정'));
      await tester.pumpAndSettle();
      await tapInSettings(tester, find.byTooltip('$label 콤보 동작'));
    }

    await openMenu('Saved');
    await tester.tap(find.text('콤보 편집'));
    await tester.pumpAndSettle();
    await tester.enterText(find.widgetWithText(TextField, '콤보 이름'), 'Edited');
    await typeKeys(tester, 'pn');
    await tester.tap(find.text('저장'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('적용'));
    await tester.pumpAndSettle();
    final edited =
        (await TerminalShortcutStore().read())[SessionType.tmux]!.combos.single;
    expect(edited.chord!.label, 'Ctrl+p+n');
    await openMenu('Edited');
    await tester.tap(find.text('콤보 삭제'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('적용'));
    await tester.pumpAndSettle();
    expect(
      (await TerminalShortcutStore().read())[SessionType.tmux]!.combos,
      isEmpty,
    );
    expect(find.text('단축키 추가'), findsOneWidget);
  });

  testWidgets('combos copy across session types once and keep new ids', (
    tester,
  ) async {
    const prefix = TerminalShortcutChord(keys: ['b'], ctrl: true);
    await _saveCombos([
      TerminalShortcut.combo('combo-prefix', 'Prefix', prefix),
      TerminalShortcut.combo(
        'combo-split',
        'Split',
        const TerminalShortcutChord(keys: ['a', 'b'], ctrl: true),
      ),
    ]);
    await _saveCombos([
      TerminalShortcut.combo('combo-zprefix', 'Mine', prefix),
    ], session: SessionType.zellij);
    await _pumpBar(tester, session: SessionType.zellij);
    await tester.tap(find.byTooltip('zellij 단축키 설정'));
    await tester.pumpAndSettle();
    final tmuxSource = find.widgetWithText(ListTile, 'tmux 설정');
    await tester.scrollUntilVisible(
      tmuxSource,
      200,
      scrollable: settingsScrollable(),
    );
    expect(find.text('바에 띄운 기본 세팅 0개 · 콤보 2개'), findsOneWidget);
    // 설정이 빈 가져오기 후보는 terminal과 herdr 둘이다
    expect(find.text('바에 띄운 단축키가 없어요'), findsNWidgets(2));
    await tapInSettings(tester, tmuxSource);
    await tester.tap(find.text('합치기'));
    await tester.pumpAndSettle();
    expect(find.text('tmux 설정에서 1개를 가져왔어요'), findsOneWidget);
    await tester.tap(tmuxSource);
    await tester.pumpAndSettle();
    await tester.tap(find.text('합치기'));
    await tester.pumpAndSettle();
    expect(find.text('가져올 새 단축키가 없어요'), findsOneWidget);
    await tester.tap(find.text('적용'));
    await tester.pumpAndSettle();
    final profiles = await TerminalShortcutStore().read();
    final zellij = profiles[SessionType.zellij]!;
    expect(zellij.combos.map((combo) => combo.label), ['Mine', 'Split']);
    expect(zellij.ids, hasLength(2));
    expect(zellij.combos.last.id, isNot('combo-split'));
    expect(zellij.combos.last.chord!.label, 'Ctrl+a+b');
    expect(profiles[SessionType.tmux]!.combos, hasLength(2));
  });

  testWidgets(
    'overwrite import copies checked defaults and skips missing ones',
    (tester) async {
      final store = TerminalShortcutStore();
      final profiles = await store.read();
      profiles[SessionType.zellij] = TerminalShortcutProfile(
        ids: ['toggle-floating', 'combo-pane', 'escape'],
        combos: [TerminalShortcut.combo('combo-pane', 'Pane', _copy)],
        removedDefaultIds: ['tab'],
      );
      profiles[SessionType.tmux] = TerminalShortcutProfile(
        ids: ['combo-old'],
        combos: [
          TerminalShortcut.combo(
            'combo-old',
            'Old',
            const TerminalShortcutChord(keys: ['o'], ctrl: true),
          ),
        ],
      );
      await store.write(profiles);
      await _pumpBar(tester);
      await tester.tap(find.byTooltip('tmux 단축키 설정'));
      await tester.pumpAndSettle();
      await tapInSettings(tester, find.widgetWithText(ListTile, 'zellij 설정'));
      await tester.tap(find.text('덮어쓰기'));
      await tester.pumpAndSettle();
      expect(find.text('zellij 설정으로 덮어썼어요'), findsOneWidget);
      await tester.tap(find.text('적용'));
      await tester.pumpAndSettle();
      final tmux = (await TerminalShortcutStore().read())[SessionType.tmux]!;
      expect(tmux.combos.map((combo) => combo.label), ['Pane']);
      expect(tmux.combos.single.id, isNot('combo-pane'));
      expect(tmux.ids, [tmux.combos.single.id, 'escape']);
      expect(tmux.removedDefaultIds, ['tab']);
    },
  );

  testWidgets(
    'deleting defaults hides them until restore while combos survive',
    (tester) async {
      await _saveCombos(
        [TerminalShortcut.combo('combo-copy', 'Copy', _copy)],
        ids: ['split-right', 'combo-copy'],
      );
      await _pumpBar(tester);
      await tester.tap(find.byTooltip('tmux 단축키 설정'));
      await tester.pumpAndSettle();
      final settings = find.byType(TerminalShortcutSettingsPage);
      Finder inSettings(Finder finder) =>
          find.descendant(of: settings, matching: finder);
      expect(inSettings(find.text('기본')), findsWidgets);
      expect(inSettings(find.text('콤보')), findsOneWidget);
      expect(
        tester
            .widget<TextButton>(find.widgetWithText(TextButton, '기본값 복원'))
            .onPressed,
        isNull,
      );
      expect(inSettings(find.text('좌우 분할')), findsNWidgets(2));
      await tester.tap(find.byTooltip('좌우 분할 기본 세팅 동작'));
      await tester.pumpAndSettle();
      expect(find.text('콤보 편집'), findsNothing);
      await tester.tap(find.text('기본 세팅 삭제'));
      await tester.pumpAndSettle();
      expect(inSettings(find.text('좌우 분할')), findsNothing);
      await tester.tap(find.text('적용'));
      await tester.pumpAndSettle();
      var tmux = (await TerminalShortcutStore().read())[SessionType.tmux]!;
      expect(tmux.removedDefaultIds, ['split-right']);
      expect(tmux.ids, ['combo-copy']);
      expect(find.widgetWithText(TextButton, '좌우 분할'), findsNothing);

      await tester.tap(find.byTooltip('tmux 단축키 설정'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('기본값 복원'));
      await tester.pumpAndSettle();
      expect(find.text('지운 기본 세팅 1개가 다시 생겨요. 내 콤보는 그대로예요.'), findsOneWidget);
      await tester.tap(find.text('복원'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('적용'));
      await tester.pumpAndSettle();
      tmux = (await TerminalShortcutStore().read())[SessionType.tmux]!;
      expect(tmux.removedDefaultIds, isEmpty);
      expect(tmux.ids, ['combo-copy']);
      expect(tmux.combos.single.id, 'combo-copy');
      expect(
        tmux.remainingDefaults(SessionType.tmux).map((key) => key.id),
        contains('split-right'),
      );
    },
  );

  testWidgets('terminal settings list only keys without session actions', (
    tester,
  ) async {
    await _pumpBar(tester, session: SessionType.terminal);
    await tester.tap(find.byTooltip('terminal 단축키 설정'));
    await tester.pumpAndSettle();
    final settings = find.byType(TerminalShortcutSettingsPage);
    expect(
      find.descendant(of: settings, matching: find.text('Esc')),
      findsOneWidget,
    );
    await tester.scrollUntilVisible(
      find.text('콤보 추가'),
      200,
      scrollable: settingsScrollable(),
    );
    expect(
      find.descendant(of: settings, matching: find.text('좌우 분할')),
      findsNothing,
    );
  });

  for (final screen in ['bar', 'editor', 'settings']) {
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
        'bar' => TerminalSoftKeyboard(
          terminal: xterm.Terminal(),
          controller: controller,
          sessionType: SessionType.tmux,
          child: const SizedBox(),
        ),
        'editor' => const TerminalShortcutComboEditor(),
        _ => const TerminalShortcutSettingsPage(sessionType: SessionType.tmux),
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
                resizeToAvoidBottomInset: screen == 'bar',
                body: child,
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
    });
  }
}
