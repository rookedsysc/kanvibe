import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

import 'app_translations.dart';

class AppLocalizations {
  const AppLocalizations(this.locale);

  final Locale locale;

  static const supportedLocales = [Locale('ko'), Locale('en'), Locale('zh')];
  static const delegate = _AppLocalizationsDelegate();

  String translate(String korean, [Map<String, Object> arguments = const {}]) {
    final translations = switch (locale.languageCode) {
      'en' => englishTranslations,
      'zh' => chineseTranslations,
      _ => const <String, String>{},
    };
    var text = translations[korean] ?? korean;
    for (final entry in arguments.entries) {
      text = text.replaceAll('{${entry.key}}', entry.value.toString());
    }
    return text;
  }
}

class _AppLocalizationsDelegate
    extends LocalizationsDelegate<AppLocalizations> {
  const _AppLocalizationsDelegate();

  @override
  bool isSupported(Locale locale) => AppLocalizations.supportedLocales.any(
    (supported) => supported.languageCode == locale.languageCode,
  );

  @override
  Future<AppLocalizations> load(Locale locale) =>
      SynchronousFuture(AppLocalizations(locale));

  @override
  bool shouldReload(_AppLocalizationsDelegate old) => false;
}

extension AppTranslation on BuildContext {
  String tr(String korean, [Map<String, Object> arguments = const {}]) =>
      (Localizations.of<AppLocalizations>(this, AppLocalizations) ??
              const AppLocalizations(Locale('ko')))
          .translate(korean, arguments);
}
