import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../../../common/providers.dart';
import '../../board/domain/task_status.dart';
import '../application/notification_settings_controller.dart';

class NotificationSettingsScreen extends ConsumerStatefulWidget {
  const NotificationSettingsScreen({super.key});

  @override
  ConsumerState<NotificationSettingsScreen> createState() =>
      _NotificationSettingsScreenState();
}

class _NotificationSettingsScreenState
    extends ConsumerState<NotificationSettingsScreen> {
  Future<void> _update({bool? isEnabled, Set<String>? enabledStatuses}) async {
    final saved = await ref
        .read(notificationSettingsProvider.notifier)
        .save(isEnabled: isEnabled, enabledStatuses: enabledStatuses);
    if (!mounted || saved) return;
    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(content: Text('설정을 저장하지 못했습니다. 다시 시도해 주세요.')),
    );
  }

  @override
  Widget build(BuildContext context) {
    final settings = ref.watch(notificationSettingsProvider);
    final connection = ref
        .watch(connectionControllerProvider)
        .value
        ?.activeConnection;
    return Scaffold(
      appBar: AppBar(title: const Text('알림 설정')),
      body: SafeArea(
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 640),
            child: settings.when(
              loading: () => const Center(child: CircularProgressIndicator()),
              error: (_, _) => Center(
                child: Padding(
                  padding: const EdgeInsets.all(AppSizes.p24),
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      const Text(
                        '알림 설정을 불러오지 못했습니다.',
                        textAlign: TextAlign.center,
                      ),
                      const SizedBox(height: AppSizes.p8),
                      const Text(
                        'PC 연결과 데스크탑 앱 버전을 확인해 주세요.',
                        textAlign: TextAlign.center,
                      ),
                      const SizedBox(height: AppSizes.p16),
                      TextButton(
                        onPressed: () =>
                            ref.invalidate(notificationSettingsProvider),
                        child: const Text('다시 시도'),
                      ),
                    ],
                  ),
                ),
              ),
              data: (preferences) => ListView(
                padding: const EdgeInsets.all(AppSizes.p16),
                children: [
                  Text(
                    connection?.displayName ?? '연결된 PC',
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  const SizedBox(height: AppSizes.p8),
                  const Text('이 PC의 데스크탑과 모바일에 함께 적용됩니다.'),
                  const SizedBox(height: AppSizes.p24),
                  Semantics(
                    identifier: 'notification-enabled',
                    child: SwitchListTile.adaptive(
                      activeTrackColor: AppColors.brandPrimary,
                      contentPadding: EdgeInsets.zero,
                      title: const Text('상태 변경 알림'),
                      subtitle: const Text('선택한 상태가 되면 알림을 받습니다.'),
                      value: preferences.isEnabled,
                      onChanged: preferences.isSaving
                          ? null
                          : (value) => _update(isEnabled: value),
                    ),
                  ),
                  const SizedBox(height: AppSizes.p24),
                  Text(
                    '알림을 받을 상태',
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  const SizedBox(height: AppSizes.p8),
                  Material(
                    color: AppColors.bgSurface,
                    child: Column(
                      children: [
                        for (final status in TaskStatus.values)
                          Semantics(
                            identifier:
                                'notification-status-${status.wireValue}',
                            child: SwitchListTile.adaptive(
                              activeTrackColor: preferences.isEnabled
                                  ? AppColors.brandPrimary
                                  : null,
                              title: Text(status.label),
                              subtitle: Text(_statusDescriptions[status]!),
                              value: preferences.enabledStatuses.contains(
                                status.wireValue,
                              ),
                              onChanged:
                                  !preferences.isEnabled || preferences.isSaving
                                  ? null
                                  : (value) {
                                      final selected = {
                                        ...preferences.enabledStatuses,
                                      };
                                      if (value) {
                                        selected.add(status.wireValue);
                                      } else {
                                        selected.remove(status.wireValue);
                                      }
                                      _update(enabledStatuses: selected);
                                    },
                            ),
                          ),
                      ],
                    ),
                  ),
                  const SizedBox(height: AppSizes.p16),
                  Text(
                    preferences.isSaving
                        ? '저장 중…'
                        : preferences.enabledStatuses.isEmpty
                        ? '선택한 상태가 없어 알림을 받지 않습니다.'
                        : '변경사항은 자동으로 저장됩니다.',
                    semanticsLabel: preferences.isSaving ? '알림 설정 저장 중' : null,
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

const _statusDescriptions = {
  TaskStatus.todo: '작업 대기',
  TaskStatus.progress: '작업 진행 중',
  TaskStatus.pending: '응답이나 입력 필요',
  TaskStatus.review: '결과 검토 필요',
  TaskStatus.done: '작업 완료',
};
