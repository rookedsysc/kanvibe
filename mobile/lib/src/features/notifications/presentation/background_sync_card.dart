import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../../../common/l10n/app_localizations.dart';
import '../../../common/network/desktop_client.dart';
import '../../../common/providers.dart';
import '../application/background_sync_controller.dart';
import '../domain/background_sync_status.dart';
import 'sync_review_button.dart';

class BackgroundSyncCard extends ConsumerWidget {
  const BackgroundSyncCard({
    this.title,
    this.buttonIdentifier = 'background-sync-start',
    this.canStart = true,
    super.key,
  });

  final String? title;
  final String buttonIdentifier;
  final bool canStart;

  String _message(BackgroundSyncStatus status) => switch (status.phase) {
    'worktrees' => 'worktree 탐색 중',
    'pull-requests' => 'PR 상태 확인 중',
    'pulls' => '변경 가져오는 중',
    'completed' when status.reviewNeeded => '동기화 완료 · 탐지 항목을 눌러 결과를 확인하세요.',
    'completed' when status.boardUpdated => '동기화가 끝나 보드를 갱신했습니다.',
    'completed' => '동기화가 끝났습니다. 새 변경 사항은 없습니다.',
    'failed' => '동기화 실패',
    _ => '새 worktree와 PR 상태를 확인하고 활성 작업을 갱신합니다. 검토할 결과는 알림함에서 확인하세요.',
  };

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final operation = ref.watch(backgroundSyncProvider);
    final connection = ref.watch(
      connectionControllerProvider.select(
        (value) => value.value?.activeConnection,
      ),
    );
    final previous = operation.value;
    final current = previous?.belongsTo(connection) == true ? previous : null;
    final status = current?.status;
    final loadError = operation.isLoading || connection == null
        ? null
        : operation.error;
    final isBusy =
        operation.isLoading ||
        (loadError == null &&
            (current?.isStarting == true || status?.isRunning == true));
    final error = loadError ?? current?.requestError;
    final hasFailure = error != null || status?.phase == 'failed';
    final message = connection == null && !operation.isLoading
        ? '연결된 PC가 없습니다.'
        : loadError != null
        ? '동기화 상태를 불러오지 못했습니다.'
        : current == null
        ? '동기화 상태 확인 중…'
        : current.isStarting == true
        ? '동기화 시작 중…'
        : _message(status!);
    final errorMessage = error is DesktopRequestException
        ? error.message
        : error != null
        ? 'PC에 요청하지 못했습니다. 네트워크를 확인해 주세요.'
        : status?.error;
    return Semantics(
      identifier: 'background-sync-status',
      liveRegion: true,
      child: Card(
        child: Padding(
          padding: const EdgeInsets.all(AppSizes.p16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(
                children: [
                  ExcludeSemantics(
                    child: Icon(
                      Icons.sync,
                      color: context.colors.textSecondary,
                    ),
                  ),
                  const SizedBox(width: AppSizes.p8),
                  Expanded(
                    child: Text(
                      title ?? context.tr('백그라운드 동기화'),
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                  ),
                ],
              ),
              const SizedBox(height: AppSizes.p8),
              Text(
                context.tr(message),
                style: TextStyle(
                  color: hasFailure
                      ? Theme.of(context).colorScheme.error
                      : context.colors.textSecondary,
                ),
              ),
              if (status?.isRunning == true) ...[
                const SizedBox(height: AppSizes.p12),
                LinearProgressIndicator(
                  value:
                      status!.completedStages / BackgroundSyncStatus.stageCount,
                  semanticsLabel: context.tr('{count} / 3 단계 완료', {
                    'count': status.completedStages,
                  }),
                ),
                const SizedBox(height: AppSizes.p8),
                Text(
                  context.tr('{count} / 3 단계 완료', {
                    'count': status.completedStages,
                  }),
                  style: Theme.of(context).textTheme.labelMedium,
                ),
                Text(
                  context.tr('다른 화면에서도 계속 진행됩니다.'),
                  style: Theme.of(context).textTheme.bodySmall,
                ),
              ],
              if (errorMessage != null) ...[
                const SizedBox(height: AppSizes.p8),
                Text(
                  context.tr(errorMessage),
                  style: TextStyle(color: Theme.of(context).colorScheme.error),
                ),
              ],
              const SizedBox(height: AppSizes.p12),
              const SyncReviewButton(),
              Semantics(
                identifier: buttonIdentifier,
                child: OutlinedButton.icon(
                  onPressed: !canStart || isBusy || connection == null
                      ? null
                      : operation.hasError
                      ? () => ref.invalidate(backgroundSyncProvider)
                      : () => ref.read(backgroundSyncProvider.notifier).start(),
                  icon: Icon(hasFailure ? Icons.refresh : Icons.sync),
                  label: Text(
                    context.tr(
                      isBusy
                          ? '동기화 처리 중…'
                          : hasFailure
                          ? '다시 시도'
                          : '지금 동기화',
                    ),
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
