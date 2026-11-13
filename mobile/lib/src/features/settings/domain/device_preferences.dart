enum AppThemePreference { system, light, dark }

enum AppLanguage { ko, en, zh }

class DevicePreferences {
  const DevicePreferences({
    this.theme = AppThemePreference.system,
    this.language = AppLanguage.ko,
  });

  final AppThemePreference theme;
  final AppLanguage language;

  DevicePreferences copyWith({
    AppThemePreference? theme,
    AppLanguage? language,
  }) => DevicePreferences(
    theme: theme ?? this.theme,
    language: language ?? this.language,
  );
}
