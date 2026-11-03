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
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/terminal_shortcut_settings_sheet.dart';

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
            const TerminalShortcutChord(key: 'tab'),
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
              const TerminalShortcutChord(key: 'tab'),
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
              const TerminalShortcutChord(key: 'escape'),
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
        tester.getSize(find.widgetWithText(TextButton, '콤보 추가')).height,
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
          home: const Scaffold(
            body: TerminalShortcutSettingsSheet(sessionType: SessionType.tmux),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byType(SwitchListTile));
    await tester.tap(find.text('적용'));
    await tester.pumpAndSettle();
    expect(find.text('저장하지 못했습니다. 다시 시도해 주세요.'), findsOneWidget);
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
