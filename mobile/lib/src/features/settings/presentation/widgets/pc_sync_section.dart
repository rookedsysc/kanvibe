import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/constants/app_sizes.dart';
import '../../../../common/l10n/app_localizations.dart';
import '../../application/pc_settings_controller.dart';
import '../../domain/pc_settings.dart';
import 'pc_request_failure.dart';

class PcSyncSection extends ConsumerWidget {
  const PcSyncSection({super.key});
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final operation = ref.watch(pcOperationsProvider);
    final saving = ref.watch(pcSettingsProvider).value?.isSaving == true;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(AppSizes.p16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              context.tr('PC 동기화'),
              style: Theme.of(context).textTheme.titleMedium,
            ),
            const SizedBox(height: AppSizes.p8),
            Text(
              context.tr(
                '새 worktree와 PR 상태를 확인하고 활성 작업을 갱신합니다. 검토할 결과는 알림함에서 확인하세요.',
              ),
            ),
            const SizedBox(height: AppSizes.p16),
            Semantics(
              identifier: 'pc-sync-now',
              child: OutlinedButton.icon(
                onPressed: operation.isLoading || saving
                    ? null
                    : () => ref.read(pcOperationsProvider.notifier).runSync(),
                icon: const Icon(Icons.sync),
                label: Text(
                  context.tr(operation.isLoading ? '동기화 처리 중…' : '지금 동기화'),
                ),
              ),
            ),
            if (operation.hasError)
              Text(
                context.tr(pcRequestError(operation.error)),
                style: TextStyle(color: Theme.of(context).colorScheme.error),
              ),
            if (operation.value case PcSyncRunResult(
              :final reviewNeeded,
              :final boardUpdated,
            ))
              Padding(
                padding: const EdgeInsets.only(top: AppSizes.p12),
                child: Text(
                  context.tr(
                    reviewNeeded
                        ? '동기화가 끝났습니다. 알림함에서 검토할 결과를 확인해 주세요.'
                        : boardUpdated
                        ? '동기화가 끝나 보드를 갱신했습니다.'
                        : '동기화가 끝났습니다. 새 변경 사항은 없습니다.',
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}
