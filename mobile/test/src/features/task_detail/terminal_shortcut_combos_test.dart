import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/constants/app_theme.dart';
import 'package:kanvibe_mobile/src/features/task_create/domain/session_type.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/terminal_shortcut_bar.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/terminal_soft_keyboard.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/terminal_shortcut_settings_sheet.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/terminal_shortcut_combo_editor.dart';
import 'package:kanvibe_mobile/src/features/task_detail/data/terminal_shortcut_store.dart';
import 'package:kanvibe_mobile/src/features/task_detail/domain/terminal_shortcut.dart';
import 'package:xterm/xterm.dart' as xterm;

void main() {
  setUp(() => FlutterSecureStorage.setMockInitialValues({}));
  testWidgets('shortcut accessory stays within40dp at normal keyboard scale', (
    tester,
  ) async {
    await tester.pumpWidget(
      ProviderScope(
        child: MaterialApp(
          theme: buildAppTheme(),
          home: Scaffold(
            body: Align(
              alignment: Alignment.bottomCenter,
              child: TerminalShortcutBar(
                sessionType: SessionType.tmux,
                onShortcut: (_) {},
              ),
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(
      tester.getSize(find.byType(TerminalShortcutBar)).height,
      lessThanOrEqualTo(40),
    );
  });
  test('legacy profiles keep key order and default to no combos', () async {
    FlutterSecureStorage.setMockInitialValues({
      'kanvibe_terminal_shortcuts_v1':
          '{"tmux":{"ids":["tab","escape"],"enabled":false}}',
    });
    final profile = (await TerminalShortcutStore().read())[SessionType.tmux]!;
    expect(profile.ids, ['tab', 'escape']);
    expect(profile.enabled, isFalse);
    expect(profile.combos, isEmpty);
  });

  test(
    'combo strokes round trip with session isolation and hidden definitions',
    () async {
      final store = TerminalShortcutStore();
      final profiles = await store.read();
      final combo = TerminalShortcut.combo('combo-retry', 'Retry', const [
        TerminalShortcutStroke(
          key: 'character',
          character: 'c',
          ctrl: true,
          alt: true,
        ),
        TerminalShortcutStroke(key: 'up'),
        TerminalShortcutStroke(key: 'enter'),
      ]);
      profiles[SessionType.tmux] = TerminalShortcutProfile(
        ids: ['tab', combo.id],
        combos: [combo],
      );
      await store.write(profiles);
      final restored = await TerminalShortcutStore().read();
      expect(restored[SessionType.tmux]!.ids, ['tab', 'combo-retry']);
      final loaded = restored[SessionType.tmux]!.combos.single;
      expect(loaded.description, 'Ctrl+Alt+c → ↑ → Enter');
      expect(loaded.strokes.map((stroke) => stroke.key), [
        'character',
        'up',
        'enter',
      ]);
      expect(restored[SessionType.zellij]!.combos, isEmpty);
      profiles[SessionType.tmux] = TerminalShortcutProfile(
        ids: [],
        combos: [combo],
      );
      await store.write(profiles);
      expect((await store.read())[SessionType.tmux]!.combos, hasLength(1));
      expect((await store.read())[SessionType.tmux]!.ids, isEmpty);
    },
  );

  test(
    'malformed combo definitions and references cannot reach execution',
    () async {
      final valid = {
        'id': 'combo-good',
        'label': 'Good',
        'strokes': [
          {'key': 'enter'},
        ],
      };
      FlutterSecureStorage.setMockInitialValues({
        'kanvibe_terminal_shortcuts_v1': jsonEncode({
          'tmux': {
            'ids': [
              'combo-good',
              'combo-bad',
              'combo-action',
              'combo-empty',
              'combo-unknown',
              'combo-space',
            ],
            'combos': [
              valid,
              valid,
              {
                'id': 'combo-bad',
                'label': 'Bad',
                'strokes': [
                  {'key': 'character', 'character': 'two'},
                ],
              },
              {
                'id': 'combo-action',
                'label': 'Bad',
                'strokes': [
                  {'key': 'split-right'},
                ],
              },
              {'id': 'combo-empty', 'label': 'Bad', 'strokes': []},
              {
                'id': 'combo-unknown',
                'label': 'Bad',
                'strokes': [
                  {'key': 'not-a-key'},
                ],
              },
              {
                'id': 'combo-space',
                'label': 'Space',
                'strokes': [
                  {'key': 'character', 'character': ' ', 'ctrl': true},
                ],
              },
            ],
          },
        }),
      });
      final profile = (await TerminalShortcutStore().read())[SessionType.tmux]!;
      expect(profile.ids, ['combo-good', 'combo-space']);
      expect(profile.combos.map((combo) => combo.id), [
        'combo-good',
        'combo-space',
      ]);
    },
  );

  testWidgets(
    'one press sends ordered chords and consumes previous modifiers',
    (tester) async {
      final store = TerminalShortcutStore();
      final profiles = await store.read();
      final combo = TerminalShortcut.combo('combo-run', 'Retry', const [
        TerminalShortcutStroke(key: 'character', character: 'x'),
        TerminalShortcutStroke(
          key: 'character',
          character: 'c',
          ctrl: true,
          alt: true,
        ),
        TerminalShortcutStroke(key: 'up'),
        TerminalShortcutStroke(key: 'enter'),
      ]);
      profiles[SessionType.tmux] = TerminalShortcutProfile(
        ids: ['ctrl', combo.id],
        combos: [combo],
      );
      await store.write(profiles);
      final output = <String>[];
      final terminal = xterm.Terminal()..onOutput = output.add;
      terminal.write('\x1b[?1h');
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
                child: const ColoredBox(color: Colors.black),
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(TextButton, 'Ctrl'));
      await tester.tap(find.widgetWithText(TextButton, 'Retry'));
      await tester.pump();
      expect(output.join(), 'x\x1b\x03\x1bOA\r');
      await tester.tap(find.widgetWithText(TextButton, 'Retry'));
      expect(output.join(), 'x\x1b\x03\x1bOA\rx\x1b\x03\x1bOA\r');
    },
  );

  testWidgets('combo editor validates name and can add key steps', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        theme: buildAppTheme(),
        home: const Scaffold(body: TerminalShortcutComboEditor()),
      ),
    );
    await tester.tap(find.text('콤보 저장'));
    await tester.pump();
    expect(find.textContaining('이름과 각 단계의 키'), findsOneWidget);
    await tester.enterText(find.byType(TextField).first, 'Retry');
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.textContaining('키 단계 추가'));
    await tester.pumpAndSettle();
    await tester.tap(find.textContaining('키 단계 추가'));
    await tester.pump();
    expect(find.text('2번째 키'), findsOneWidget);
    expect(find.byTooltip('1번째 키 삭제'), findsOneWidget);
    await tester.tap(find.byTooltip('1번째 키 삭제'));
    await tester.pump();
    expect(find.text('2번째 키'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets(
    'combo draft cancels, persists on Apply, then edits and deletes',
    (tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            theme: buildAppTheme(),
            home: Scaffold(
              body: Align(
                alignment: Alignment.bottomCenter,
                child: TerminalShortcutBar(
                  sessionType: SessionType.tmux,
                  onShortcut: (_) {},
                ),
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      Future<void> addCombo(String name) async {
        await tester.tap(find.textContaining('콤보 추가 ('));
        await tester.pumpAndSettle();
        await tester.enterText(find.widgetWithText(TextField, '콤보 이름'), name);
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
            .label,
        'Saved',
      );
      await tester.tap(find.byTooltip('tmux 단축키 설정'));
      await tester.pumpAndSettle();
      await tester.scrollUntilVisible(
        find.byTooltip('Saved 콤보 동작'),
        300,
        scrollable: find
            .descendant(
              of: find.byType(TerminalShortcutSettingsSheet),
              matching: find.byType(Scrollable),
            )
            .first,
      );
      await tester.tap(find.byTooltip('Saved 콤보 동작'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('콤보 편집'));
      await tester.pumpAndSettle();
      await tester.enterText(find.widgetWithText(TextField, '콤보 이름'), 'Edited');
      await tester.tap(find.text('콤보 저장'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('적용'));
      await tester.pumpAndSettle();
      expect(
        (await TerminalShortcutStore().read())[SessionType.tmux]!
            .combos
            .single
            .label,
        'Edited',
      );
      await tester.tap(find.byTooltip('tmux 단축키 설정'));
      await tester.pumpAndSettle();
      await tester.scrollUntilVisible(
        find.byTooltip('Edited 콤보 동작'),
        300,
        scrollable: find
            .descendant(
              of: find.byType(TerminalShortcutSettingsSheet),
              matching: find.byType(Scrollable),
            )
            .first,
      );
      await tester.tap(find.byTooltip('Edited 콤보 동작'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('콤보 삭제'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('적용'));
      await tester.pumpAndSettle();
      expect(
        (await TerminalShortcutStore().read())[SessionType.tmux]!.combos,
        isEmpty,
      );
    },
  );
  testWidgets(
    'combo editor remains scrollable with large text and keyboard insets',
    (tester) async {
      await tester.pumpWidget(
        MaterialApp(
          theme: buildAppTheme(),
          home: MediaQuery(
            data: const MediaQueryData(
              size: Size(320, 640),
              viewInsets: EdgeInsets.only(bottom: 250),
              textScaler: TextScaler.linear(2),
            ),
            child: Scaffold(
              body: SizedBox(
                width: 320,
                child: TerminalShortcutComboEditor(
                  combo: TerminalShortcut.combo('combo-large', 'Large', const [
                    TerminalShortcutStroke(
                      key: 'backspace',
                      ctrl: true,
                      alt: true,
                    ),
                  ]),
                ),
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      expect(find.text('콤보 저장'), findsOneWidget);
    },
  );
}
