import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../../../common/l10n/app_localizations.dart';
import '../application/device_preferences_controller.dart';
import '../domain/device_preferences.dart';

class DevicePreferencesScreen extends ConsumerWidget {
  const DevicePreferencesScreen({super.key});

  Future<void> _showSaveResult(
    BuildContext context,
    Future<bool> action,
  ) async {
    final saved = await action;
    if (saved || !context.mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(context.tr('설정을 저장하지 못했습니다. 다시 시도해 주세요.'))),
    );
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final preferences = ref.watch(devicePreferencesProvider);
    return Scaffold(
      appBar: AppBar(title: Text(context.tr('기기 설정'))),
      body: SafeArea(
        child: preferences.when(
          loading: () => const Center(child: CircularProgressIndicator()),
          error: (_, _) => Center(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(context.tr('설정을 불러오지 못했습니다.')),
                TextButton(
                  onPressed: () => ref.invalidate(devicePreferencesProvider),
                  child: Text(context.tr('다시 시도')),
                ),
              ],
            ),
          ),
          data: (state) => Align(
            alignment: Alignment.topCenter,
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 560),
              child: ListView(
                padding: const EdgeInsets.all(AppSizes.p16),
                children: [
                  Text(
                    context.tr('이 기기에만 적용됩니다. 연결된 PC 설정은 바뀌지 않습니다.'),
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                  const SizedBox(height: AppSizes.p24),
                  Text(
                    context.tr('화면 테마'),
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  const SizedBox(height: AppSizes.p8),
                  RadioGroup<AppThemePreference>(
                    groupValue: state.preferences.theme,
                    onChanged: (theme) {
                      if (theme == null || state.isSaving) return;
                      _showSaveResult(
                        context,
                        ref
                            .read(devicePreferencesProvider.notifier)
                            .setTheme(theme),
                      );
                    },
                    child: Column(
                      children: [
                        for (final theme in AppThemePreference.values)
                          Semantics(
                            identifier: 'theme-${theme.name}',
                            child: RadioListTile<AppThemePreference>(
                              value: theme,
                              enabled: !state.isSaving,
                              title: Text(context.tr(_themeLabels[theme]!)),
                              tileColor: context.colors.bgSurface,
                            ),
                          ),
                      ],
                    ),
                  ),
                  const SizedBox(height: AppSizes.p24),
                  Text(
                    context.tr('언어'),
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  const SizedBox(height: AppSizes.p8),
                  RadioGroup<AppLanguage>(
                    groupValue: state.preferences.language,
                    onChanged: (language) {
                      if (language == null || state.isSaving) return;
                      _showSaveResult(
                        context,
                        ref
                            .read(devicePreferencesProvider.notifier)
                            .setLanguage(language),
                      );
                    },
                    child: Column(
                      children: [
                        for (final language in AppLanguage.values)
                          Semantics(
                            identifier: 'language-${language.name}',
                            child: RadioListTile<AppLanguage>(
                              value: language,
                              enabled: !state.isSaving,
                              title: Text(_languageLabels[language]!),
                              tileColor: context.colors.bgSurface,
                            ),
                          ),
                      ],
                    ),
                  ),
                  if (state.isSaving) ...[
                    const SizedBox(height: AppSizes.p16),
                    Semantics(
                      liveRegion: true,
                      child: Text(context.tr('저장 중…')),
                    ),
                  ],
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

const _themeLabels = {
  AppThemePreference.system: '시스템 설정 따르기',
  AppThemePreference.light: '라이트',
  AppThemePreference.dark: '다크',
};

const _languageLabels = {
  AppLanguage.ko: '한국어',
  AppLanguage.en: 'English',
  AppLanguage.zh: '中文',
};
