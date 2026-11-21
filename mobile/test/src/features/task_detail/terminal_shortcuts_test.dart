import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/constants/app_theme.dart';
import 'package:kanvibe_mobile/src/features/task_create/domain/session_type.dart';
import 'package:kanvibe_mobile/src/features/task_detail/application/terminal_shortcut_controller.dart';
import 'package:kanvibe_mobile/src/features/task_detail/data/terminal_shortcut_store.dart';
import 'package:kanvibe_mobile/src/features/task_detail/domain/mirror_pane.dart';
import 'package:kanvibe_mobile/src/features/task_detail/domain/terminal_shortcut.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/terminal_shortcut_bar.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/terminal_shortcut_settings_page.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUp(() => FlutterSecureStorage.setMockInitialValues({}));

  test(
    'surfaces choose the reported session and unknown sessions stay generic',
    () {
      for (final type in SessionType.values) {
        expect(
          TaskSurfaces.fromJson({'sessionType': type.name}).sessionType,
          type,
        );
      }
      expect(
        TaskSurfaces.fromJson({'sessionType': 'future'}).sessionType,
        SessionType.terminal,
      );
      expect(TaskSurfaces.fromJson({}).sessionType, SessionType.terminal);
    },
  );

  test('profiles expose distinct supported multiplexer actions', () {
    final terminal = terminalShortcuts(SessionType.terminal);
    final tmux = terminalShortcuts(SessionType.tmux);
    final zellij = terminalShortcuts(SessionType.zellij);
    expect(terminal.every((key) => key.action == null), isTrue);
    expect(tmux.map((key) => key.action), contains('copy-mode'));
    expect(tmux.map((key) => key.action), isNot(contains('toggle-floating')));
    expect(zellij.map((key) => key.action), contains('toggle-floating'));
    expect(zellij.map((key) => key.action), isNot(contains('copy-mode')));
  });

  test('Herdr는 분할과 확대만 제공하고 지난 출력은 손가락으로 읽는다', () {
    final actions = terminalShortcuts(SessionType.herdr)
        .map((key) => key.action)
        .whereType<String>()
        .toList();
    expect(actions, ['split-right', 'split-down', 'toggle-fullscreen']);
  });

  test('tmux와 Zellij의 스크롤 단축키는 그대로 남는다', () {
    for (final type in [SessionType.tmux, SessionType.zellij]) {
      expect(
        terminalShortcuts(type).map((key) => key.action),
        containsAll(['scroll-up', 'scroll-down', 'scroll-bottom']),
      );
    }
  });

  test(
    'ordered profiles persist independently including disabled and empty',
    () async {
      final store = TerminalShortcutStore();
      final profiles = await store.read();
      profiles[SessionType.tmux] = TerminalShortcutProfile(
        enabled: false,
        ids: ['combo-tab'],
        combos: [
          TerminalShortcut.combo(
            'combo-tab',
            'Tab',
            const TerminalShortcutChord(keys: ['tab']),
          ),
        ],
      );
      profiles[SessionType.zellij] = TerminalShortcutProfile(ids: []);
      await store.write(profiles);
      final restored = await TerminalShortcutStore().read();
      expect(restored[SessionType.tmux]!.ids, ['combo-tab']);
      expect(restored[SessionType.tmux]!.enabled, isFalse);
      expect(restored[SessionType.zellij]!.ids, isEmpty);
      expect(
        restored[SessionType.terminal]!.ids,
        TerminalShortcutProfile.defaults().ids,
      );
    },
  );

  test(
    'corrupt settings fall back and unknown or duplicate IDs are removed',
    () async {
      FlutterSecureStorage.setMockInitialValues({
        'kanvibe_terminal_shortcuts_v1': '{broken',
      });
      expect(
        (await TerminalShortcutStore().read())[SessionType.tmux]!.ids,
        isEmpty,
      );
      FlutterSecureStorage.setMockInitialValues({
        'kanvibe_terminal_shortcuts_v1':
            '{"tmux":{"ids":["tab","tab","unknown",5,"escape"]}}',
      });
      expect(
        (await TerminalShortcutStore().read())[SessionType.tmux]!.ids,
        isEmpty,
      );
    },
  );

  test(
    'v3 keeps removed defaults and drops defaults the session lacks',
    () async {
      final store = TerminalShortcutStore();
      final profiles = await store.read();
      profiles[SessionType.tmux] = TerminalShortcutProfile(
        ids: ['escape', 'toggle-floating', 'copy-mode', 'tab'],
        removedDefaultIds: ['copy-mode', 'unknown'],
      );
      await store.write(profiles);
      final tmux = (await TerminalShortcutStore().read())[SessionType.tmux]!;
      expect(tmux.ids, ['escape', 'tab']);
      expect(tmux.removedDefaultIds, ['copy-mode']);
      expect(
        tmux.remainingDefaults(SessionType.tmux).map((key) => key.id),
        isNot(contains('copy-mode')),
      );
      expect(tmux.visibleShortcuts(SessionType.tmux).map((key) => key.id), [
        'escape',
        'tab',
      ]);
    },
  );

  test(
    'v3 data saved before removable defaults reads as nothing removed',
    () async {
      FlutterSecureStorage.setMockInitialValues({
        'kanvibe_terminal_shortcuts_v3': jsonEncode({
          'tmux': {'enabled': true, 'ids': <String>[], 'combos': <Object>[]},
        }),
      });
      final tmux = (await TerminalShortcutStore().read())[SessionType.tmux]!;
      expect(tmux.removedDefaultIds, isEmpty);
      expect(
        tmux.remainingDefaults(SessionType.tmux),
        hasLength(terminalShortcuts(SessionType.tmux).length),
      );
    },
  );

  group('importing another session profile', () {
    final pane = TerminalShortcut.combo(
      'combo-pane',
      'Pane',
      const TerminalShortcutChord(keys: ['p'], ctrl: true),
    );
    final source = TerminalShortcutProfile(
      ids: ['toggle-floating', 'combo-pane', 'escape', 'tab'],
      combos: [pane],
      removedDefaultIds: ['end'],
    );

    test('merge adds new chords and revives checked defaults', () {
      final target = TerminalShortcutProfile(
        enabled: false,
        ids: ['combo-same'],
        combos: [TerminalShortcut.combo('combo-same', 'Same', pane.chord!)],
        removedDefaultIds: ['tab'],
      );
      final merged = target.mergedWith(
        source,
        SessionType.tmux,
        newComboIdPrefix: 'combo-new',
      );
      expect(merged.enabled, isFalse);
      expect(merged.combos.map((combo) => combo.id), ['combo-same']);
      expect(merged.ids, ['combo-same', 'escape', 'tab']);
      expect(merged.removedDefaultIds, isEmpty);
    });

    test('merge stops copying chords at the combo limit', () {
      final full = TerminalShortcutProfile(
        ids: [],
        combos: [
          for (var index = 0; index < maxTerminalCombos; index++)
            TerminalShortcut.combo(
              'combo-$index',
              '$index',
              TerminalShortcutChord(keys: [String.fromCharCode(0x61 + index)]),
            ),
        ],
      );
      final merged = full.mergedWith(
        source,
        SessionType.tmux,
        newComboIdPrefix: 'combo-new',
      );
      expect(merged.combos, hasLength(maxTerminalCombos));
      expect(merged.ids, ['escape', 'tab']);
    });

    test('overwrite mirrors the source but keeps target-only defaults', () {
      final target = TerminalShortcutProfile(
        enabled: false,
        ids: ['copy-mode'],
        removedDefaultIds: ['home'],
      );
      final overwritten = target.overwrittenBy(
        source,
        SessionType.tmux,
        newComboIdPrefix: 'combo-new',
      );
      expect(overwritten.enabled, isFalse);
      expect(overwritten.combos.single.id, 'combo-new-0');
      expect(overwritten.ids, ['combo-new-0', 'escape', 'tab']);
      expect(overwritten.removedDefaultIds, ['end']);
      expect(
        overwritten.remainingDefaults(SessionType.tmux).map((key) => key.id),
        contains('copy-mode'),
      );
    });
  });

  test('controller preserves latest profile when saves overlap', () async {
    final container = ProviderContainer();
    addTearDown(container.dispose);
    await container.read(terminalShortcutControllerProvider.future);
    final controller = container.read(
      terminalShortcutControllerProvider.notifier,
    );
    await Future.wait([
      controller.save(
        SessionType.tmux,
        TerminalShortcutProfile(
          ids: ['combo-tab'],
          combos: [
            TerminalShortcut.combo(
              'combo-tab',
              'Tab',
              const TerminalShortcutChord(keys: ['tab']),
            ),
          ],
        ),
      ),
      controller.save(
        SessionType.zellij,
        TerminalShortcutProfile(
          ids: ['combo-escape'],
          combos: [
            TerminalShortcut.combo(
              'combo-escape',
              'Esc',
              const TerminalShortcutChord(keys: ['escape']),
            ),
          ],
        ),
      ),
    ]);
    final profiles = await TerminalShortcutStore().read();
    expect(profiles[SessionType.tmux]!.ids, ['combo-tab']);
    expect(profiles[SessionType.zellij]!.ids, ['combo-escape']);
  });

  testWidgets(
    'settings cancel discards draft; apply persists and disabled bar stays reachable',
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
      await tester.tap(find.byTooltip('tmux 단축키 설정'));
      await tester.pumpAndSettle();
      await tester.tap(find.byType(SwitchListTile));
      await tester.tap(find.byTooltip('닫기'));
      await tester.pumpAndSettle();
      expect(find.text('tmux · 단축키 꺼짐'), findsNothing);
      await tester.tap(find.byTooltip('tmux 단축키 설정'));
      await tester.pumpAndSettle();
      await tester.tap(find.byType(SwitchListTile));
      await tester.tap(find.text('적용'));
      await tester.pumpAndSettle();
      expect(find.text('tmux · 단축키 꺼짐'), findsOneWidget);
      expect(
        tester.getSemantics(find.text('tmux · 단축키 꺼짐')).label,
        'tmux · 단축키 꺼짐',
      );
      expect(find.byTooltip('tmux 단축키 설정'), findsOneWidget);
      expect(
        (await TerminalShortcutStore().read())[SessionType.tmux]!.enabled,
        isFalse,
      );
    },
  );

  testWidgets(
    'narrow viewport with large text keeps controls in their own scroll lane',
    (tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: MediaQuery(
              data: const MediaQueryData(
                size: Size(320, 640),
                textScaler: TextScaler.linear(2),
              ),
              child: Scaffold(
                body: SizedBox(
                  width: 320,
                  child: TerminalShortcutBar(
                    sessionType: SessionType.zellij,
                    onShortcut: (_) {},
                  ),
                ),
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      expect(
        tester.getSize(find.byType(IconButton)).width,
        greaterThanOrEqualTo(32),
      );
      expect(
        tester.getSize(find.widgetWithText(TextButton, '단축키 추가')).height,
        greaterThanOrEqualTo(32),
      );
    },
  );

  testWidgets('save failure preserves draft and shows retryable error', (
    tester,
  ) async {
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          terminalShortcutStoreProvider.overrideWithValue(_FailingStore()),
        ],
        child: MaterialApp(
          home: const TerminalShortcutSettingsPage(
            sessionType: SessionType.tmux,
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byType(SwitchListTile));
    await tester.tap(find.text('적용'));
    await tester.pumpAndSettle();
    expect(find.text('저장하지 못했어요. 다시 시도해 주세요.'), findsOneWidget);
    expect(
      tester.widget<SwitchListTile>(find.byType(SwitchListTile)).value,
      isFalse,
    );
    expect(find.text('적용'), findsOneWidget);
  });
}

class _FailingStore extends TerminalShortcutStore {
  @override
  Future<void> write(
    Map<SessionType, TerminalShortcutProfile> profiles,
  ) async => throw Exception('offline');
}
