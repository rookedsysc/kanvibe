import 'dart:convert';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import '../domain/device_preferences.dart';

class DevicePreferencesStore {
  DevicePreferencesStore({FlutterSecureStorage? storage})
    : _storage = storage ?? const FlutterSecureStorage();

  final FlutterSecureStorage _storage;
  static const _key = 'kanvibe_device_preferences_v1';

  Future<DevicePreferences> read() async {
    final raw = await _storage.read(key: _key);
    if (raw == null) return const DevicePreferences();
    Object? decoded;
    try {
      decoded = jsonDecode(raw);
    } on FormatException {
      return const DevicePreferences();
    }
    if (decoded is! Map) return const DevicePreferences();
    final storedTheme = decoded['theme'];
    final storedLanguage = decoded['language'];
    return DevicePreferences(
      theme:
          AppThemePreference.values
              .where((theme) => theme.name == storedTheme)
              .firstOrNull ??
          AppThemePreference.system,
      language:
          AppLanguage.values
              .where((language) => language.name == storedLanguage)
              .firstOrNull ??
          AppLanguage.ko,
    );
  }

  Future<void> write(DevicePreferences preferences) => _storage.write(
    key: _key,
    value: jsonEncode({
      'theme': preferences.theme.name,
      'language': preferences.language.name,
    }),
  );
}
