import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/app.dart';
import 'package:kanvibe_mobile/src/common/constants/app_colors.dart';
import 'package:kanvibe_mobile/src/common/constants/app_theme.dart';
import 'package:kanvibe_mobile/src/common/l10n/app_localizations.dart';
import 'package:kanvibe_mobile/src/common/l10n/app_translations.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/connection/presentation/connection_screen.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/settings/application/device_preferences_controller.dart';
import 'package:kanvibe_mobile/src/features/settings/data/device_preferences_store.dart';
import 'package:kanvibe_mobile/src/features/settings/domain/device_preferences.dart';
import 'package:kanvibe_mobile/src/features/settings/presentation/device_preferences_screen.dart';

import '../../../support/fake_desktop.dart';

class MemoryPreferencesStore extends DevicePreferencesStore {
  DevicePreferences saved = const DevicePreferences();
  bool shouldFailWrite = false;
  bool shouldFailRead = false;
  Completer<void>? writeBarrier;
  int writes = 0;

  @override
  Future<DevicePreferences> read() async {
    if (shouldFailRead) throw StateError('read failed');
    return saved;
  }

  @override
  Future<void> write(DevicePreferences preferences) async {
    writes++;
    if (writeBarrier case final barrier?) await barrier.future;
    if (shouldFailWrite) throw StateError('write failed');
    saved = preferences;
  }
}

Future<void> pumpPreferencesApp(
  WidgetTester tester,
  DevicePreferencesStore store,
) async {
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        devicePreferencesStoreProvider.overrideWithValue(store),
        connectionStoreProvider.overrideWithValue(
          FakeConnectionStore(initial: const SavedConnections()),
        ),
        desktopClientProvider.overrideWithValue(FakeDesktopClient()),
      ],
      child: const KanvibeMobileApp(),
    ),
  );
  await tester.pumpAndSettle();
}

Future<void> openPreferences(WidgetTester tester) async {
  final context = tester.element(find.byType(ConnectionScreen));
  unawaited(
    Navigator.of(context).push(
      MaterialPageRoute<void>(builder: (_) => const DevicePreferencesScreen()),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUp(() => FlutterSecureStorage.setMockInitialValues({}));

  test('theme and locale survive a new store instance', () async {
    await DevicePreferencesStore().write(
      const DevicePreferences(
        theme: AppThemePreference.light,
        language: AppLanguage.zh,
      ),
    );
    final restored = await DevicePreferencesStore().read();
    expect(restored.theme, AppThemePreference.light);
    expect(restored.language, AppLanguage.zh);
  });

  for (final raw in ['not-json', '[]', '{"theme":"unknown","language":42}']) {
    test('invalid stored preferences use supported defaults: $raw', () async {
      FlutterSecureStorage.setMockInitialValues({
        'kanvibe_device_preferences_v1': raw,
      });
      final restored = await DevicePreferencesStore().read();
      expect(restored.theme, AppThemePreference.system);
      expect(restored.language, AppLanguage.ko);
    });
  }

  test('failed save retains the last persisted preference', () async {
    final store = MemoryPreferencesStore()..shouldFailWrite = true;
    final container = ProviderContainer(
      overrides: [devicePreferencesStoreProvider.overrideWithValue(store)],
    );
    addTearDown(container.dispose);
    await container.read(devicePreferencesProvider.future);
    final saved = await container
        .read(devicePreferencesProvider.notifier)
        .setTheme(AppThemePreference.light);
    expect(saved, isFalse);
    expect(
      container.read(devicePreferencesProvider).requireValue.preferences.theme,
      AppThemePreference.system,
    );
    expect(
      container.read(devicePreferencesProvider).requireValue.isSaving,
      isFalse,
    );
  });

  test('overlapping writes cannot replace the pending preference', () async {
    final store = MemoryPreferencesStore()..writeBarrier = Completer<void>();
    final container = ProviderContainer(
      overrides: [devicePreferencesStoreProvider.overrideWithValue(store)],
    );
    addTearDown(container.dispose);
    await container.read(devicePreferencesProvider.future);
    final controller = container.read(devicePreferencesProvider.notifier);
    final pending = controller.setTheme(AppThemePreference.dark);
    expect(await controller.setLanguage(AppLanguage.en), isFalse);
    expect(store.writes, 1);
    store.writeBarrier!.complete();
    expect(await pending, isTrue);
    expect(store.saved.theme, AppThemePreference.dark);
    expect(store.saved.language, AppLanguage.ko);
  });

  test('translations preserve arguments and unknown user content', () {
    const en = AppLocalizations(Locale('en'));
    const zh = AppLocalizations(Locale('zh'));
    expect(en.translate('프로젝트'), 'Project');
    expect(zh.translate('프로젝트'), '项目');
    expect(
      en.translate('읽지 않은 알림 {count}개', {'count': 12}),
      '12 unread notifications',
    );
    expect(zh.translate('사용자가 작성한 태스크 제목'), '사용자가 작성한 태스크 제목');
    expect(englishTranslations.keys.toSet(), chineseTranslations.keys.toSet());
    expect(englishTranslations.length, greaterThanOrEqualTo(270));
  });

  test('light and dark semantic text have readable contrast', () {
    for (final palette in [AppPalette.light, AppPalette.dark]) {
      for (final foreground in [
        palette.textPrimary,
        palette.textSecondary,
        palette.textMuted,
      ]) {
        final textLuminance = foreground.computeLuminance();
        final backgroundLuminance = palette.bgSurface.computeLuminance();
        final brighter = textLuminance > backgroundLuminance
            ? textLuminance
            : backgroundLuminance;
        final darker = textLuminance < backgroundLuminance
            ? textLuminance
            : backgroundLuminance;
        expect((brighter + 0.05) / (darker + 0.05), greaterThanOrEqualTo(4.5));
      }
    }
    expect(buildAppTheme().brightness, Brightness.dark);
    expect(buildAppTheme(Brightness.light).brightness, Brightness.light);
  });

  testWidgets('theme and language apply to MaterialApp and survive restart', (
    tester,
  ) async {
    final store = DevicePreferencesStore();
    await pumpPreferencesApp(tester, store);
    await openPreferences(tester);
    await tester.tap(find.text('라이트'));
    await tester.pumpAndSettle();
    expect(
      Theme.of(tester.element(find.byType(DevicePreferencesScreen))).brightness,
      Brightness.light,
    );
    await tester.tap(find.text('English'));
    await tester.pumpAndSettle();
    expect(find.text('Device settings'), findsOneWidget);
    expect(find.text('Appearance'), findsOneWidget);
    expect(
      Localizations.localeOf(
        tester.element(find.byType(DevicePreferencesScreen)),
      ),
      const Locale('en'),
    );
    await tester.pumpWidget(const SizedBox());
    await pumpPreferencesApp(tester, DevicePreferencesStore());
    await openPreferences(tester);
    expect(find.text('Device settings'), findsOneWidget);
    expect(
      Theme.of(tester.element(find.byType(DevicePreferencesScreen))).brightness,
      Brightness.light,
    );
    await tester.tap(find.text('中文'));
    await tester.pumpAndSettle();
    expect(find.text('设备设置'), findsOneWidget);
    expect(find.text('外观'), findsOneWidget);
  });

  testWidgets(
    'system theme follows platform changes without resetting language',
    (tester) async {
      addTearDown(tester.platformDispatcher.clearPlatformBrightnessTestValue);
      final store = MemoryPreferencesStore()
        ..saved = const DevicePreferences(language: AppLanguage.en);
      tester.platformDispatcher.platformBrightnessTestValue = Brightness.light;
      await pumpPreferencesApp(tester, store);
      await openPreferences(tester);
      expect(
        Theme.of(tester.element(find.byType(DevicePreferencesScreen)))
            .brightness,
        Brightness.light,
      );
      tester.platformDispatcher.platformBrightnessTestValue = Brightness.dark;
      await tester.pumpAndSettle();
      expect(
        Theme.of(tester.element(find.byType(DevicePreferencesScreen)))
            .brightness,
        Brightness.dark,
      );
      expect(find.text('Device settings'), findsOneWidget);
    },
  );

  testWidgets('read failure can be retried and write failure stays visible', (
    tester,
  ) async {
    final store = MemoryPreferencesStore()..shouldFailRead = true;
    await pumpPreferencesApp(tester, store);
    await openPreferences(tester);
    expect(find.text('설정을 불러오지 못했습니다.'), findsOneWidget);
    store.shouldFailRead = false;
    await tester.tap(find.text('다시 시도'));
    await tester.pumpAndSettle();
    store.shouldFailWrite = true;
    await tester.tap(find.text('라이트'));
    await tester.pumpAndSettle();
    expect(find.text('설정을 저장하지 못했습니다. 다시 시도해 주세요.'), findsOneWidget);
    expect(store.saved.theme, AppThemePreference.system);
  });

  for (final size in [const Size(375, 812), const Size(812, 375)]) {
    testWidgets('preferences scroll at large text on $size', (tester) async {
      await tester.binding.setSurfaceSize(size);
      addTearDown(() => tester.binding.setSurfaceSize(null));
      tester.platformDispatcher.textScaleFactorTestValue = 2;
      addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
      await pumpPreferencesApp(tester, MemoryPreferencesStore());
      await openPreferences(tester);
      await tester.scrollUntilVisible(find.text('中文'), 100);
      await tester.pumpAndSettle();
      await tester.ensureVisible(find.text('中文'));
      await tester.pumpAndSettle();
      expect(find.text('中文').hitTestable(), findsOneWidget);
      await tester.tap(find.text('中文'));
      await tester.pumpAndSettle();
      expect(find.text('设备设置'), findsOneWidget);
      expect(tester.takeException(), isNull);
    });
  }
}
