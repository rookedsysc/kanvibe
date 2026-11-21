import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../data/device_preferences_store.dart';
import '../domain/device_preferences.dart';

final devicePreferencesStoreProvider = Provider<DevicePreferencesStore>(
  (ref) => DevicePreferencesStore(),
);

final devicePreferencesProvider =
    AsyncNotifierProvider<DevicePreferencesController, DevicePreferencesState>(
      DevicePreferencesController.new,
    );

class DevicePreferencesState {
  const DevicePreferencesState(this.preferences, {this.isSaving = false});

  final DevicePreferences preferences;
  final bool isSaving;
}

class DevicePreferencesController
    extends AsyncNotifier<DevicePreferencesState> {
  @override
  Future<DevicePreferencesState> build() async => DevicePreferencesState(
    await ref.read(devicePreferencesStoreProvider).read(),
  );

  Future<bool> setTheme(AppThemePreference theme) => _save(theme: theme);

  Future<bool> setLanguage(AppLanguage language) => _save(language: language);

  Future<bool> _save({AppThemePreference? theme, AppLanguage? language}) async {
    final current = state.value;
    if (current == null || current.isSaving) return false;
    final updated = current.preferences.copyWith(
      theme: theme,
      language: language,
    );
    state = AsyncData(
      DevicePreferencesState(current.preferences, isSaving: true),
    );
    try {
      await ref.read(devicePreferencesStoreProvider).write(updated);
      if (!ref.mounted) return false;
      state = AsyncData(DevicePreferencesState(updated));
      return true;
    } catch (_) {
      if (ref.mounted) state = AsyncData(current);
      return false;
    }
  }
}
