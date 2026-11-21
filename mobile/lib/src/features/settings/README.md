# Device preferences

`DevicePreferencesScreen` edits this device's appearance and UI language. It does not change PC settings. It reads `devicePreferencesProvider`; `DevicePreferencesController` applies a change only after `DevicePreferencesStore` saves it to secure storage. A failed save keeps the last saved value and the screen offers a retry for failed initial reads.

`AppThemePreference` supports `system`, `light`, and `dark`. `AppLanguage` supports `ko`, `en`, and `zh`. `KanvibeMobileApp` watches the shared preferences and configures MaterialApp themes, locale and framework localization delegates. System appearance follows platform brightness changes.

UI surfaces and text use `context.colors` from `common/constants/app_colors.dart`. Terminal, brand and status constants remain available through `AppColors`. `context.tr(korean, {'name': value})` from `common/l10n/app_localizations.dart` translates UI resources with named placeholders while preserving unknown content, including user-entered task titles. Resources live in `app_translations.dart`.

Stable selectors: `theme-system`, `theme-light`, `theme-dark`, `language-ko`, `language-en`, `language-zh`. They use native radio controls and must be confirmed in the device hierarchy before a required Maestro run.
