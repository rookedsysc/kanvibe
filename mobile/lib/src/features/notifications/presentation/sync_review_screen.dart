import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_sizes.dart';
import '../../../common/l10n/app_localizations.dart';
import '../../../common/network/desktop_client.dart';
import '../../../common/providers.dart';
import '../application/sync_review_controller.dart';
import '../application/sync_review_cleanup_controller.dart';

const _syncReviewDialogHeightFraction = 0.85;

Future<void> showSyncReviewDialog(
  BuildContext context,
  String notificationId,
) => showDialog<void>(
  context: context,
  builder: (dialogContext) => Dialog(
    clipBehavior: Clip.antiAlias,
    insetPadding: const EdgeInsets.all(AppSizes.p16),
    child: ConstrainedBox(
      constraints: BoxConstraints(
        maxWidth: 840,
        maxHeight:
            MediaQuery.sizeOf(dialogContext).height *
            _syncReviewDialogHeightFraction,
      ),
      child: SyncReviewScreen(notificationId: notificationId, isDialog: true),
    ),
  ),
);

class SyncReviewScreen extends ConsumerWidget {
  const SyncReviewScreen({
    required this.notificationId,
    this.isDialog = false,
    super.key,
  });
  final String notificationId;
  final bool isDialog;
  Future<void> _delete(BuildContext context, WidgetRef ref, int count) async {
    final original = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(context.tr('선택한 태스크를 삭제할까요?')),
        content: Text(
          context.tr(
            '병합된 PR의 태스크 {count}개와 연결된 리소스를 삭제합니다. 이 작업은 되돌릴 수 없습니다.',
            {'count': count},
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(context.tr('취소')),
          ),
          TextButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(context.tr('선택 항목 삭제')),
          ),
        ],
      ),
    );
    if (!context.mounted || confirmed != true) return;
    final active = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    if (active?.id != original?.id || active?.token != original?.token) return;
    final provider = syncReviewProvider(notificationId);
    final deleting = ref.read(provider.notifier).deleteSelected();
    if (isDialog &&
        context.mounted &&
        ref.read(provider).value?.isDeleting == true) {
      Navigator.of(context).pop();
    }
    await deleting;
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final provider = syncReviewProvider(notificationId);
    final review = ref.watch(provider);
    final cleanup = ref.watch(syncReviewCleanupProvider);
    final isAnotherCleanupRunning =
        cleanup != null && cleanup.notificationId != notificationId;
    return Scaffold(
      appBar: AppBar(
        title: Text(context.tr('동기화 결과 검토')),
        leading: isDialog
            ? IconButton(
                tooltip: context.tr('닫기'),
                onPressed: () => Navigator.of(context).pop(),
                icon: const Icon(Icons.close),
              )
            : null,
        actions: [
          IconButton(
            tooltip: context.tr('새로고침'),
            onPressed: review.value?.isDeleting == true
                ? null
                : () => ref.invalidate(provider),
            icon: const Icon(Icons.refresh),
          ),
        ],
      ),
      body: SafeArea(
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 840),
            child: review.when(
              loading: () => const Center(child: CircularProgressIndicator()),
              error: (error, _) => Center(
                child: Padding(
                  padding: const EdgeInsets.all(AppSizes.p24),
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Text(
                        error is DesktopRequestException
                            ? context.tr(error.message ?? '검토할 알림을 불러오지 못했습니다.')
                            : context.tr('PC에 요청하지 못했습니다. 네트워크를 확인해 주세요.'),
                      ),
                      TextButton(
                        onPressed: () => ref.invalidate(provider),
                        child: Text(context.tr('다시 시도')),
                      ),
                    ],
                  ),
                ),
              ),
              data: (value) => ListView(
                padding: const EdgeInsets.all(AppSizes.p16),
                children: [
                  Text(context.tr('백그라운드에서 정리합니다. 검토 창을 닫아도 계속 진행됩니다.')),
                  const SizedBox(height: AppSizes.p16),
                  if (value.review.isEmpty)
                    Text(context.tr('검토할 변경 사항이 없습니다.')),
                  if (value.review.registeredWorktrees.isNotEmpty) ...[
                    Text(
                      context.tr('새로 등록된 Worktree'),
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                    for (final item in value.review.registeredWorktrees)
                      ListTile(
                        leading: const Icon(Icons.account_tree_outlined),
                        title: Text('${item.projectName} · ${item.branchName}'),
                        subtitle: SelectableText(
                          '${item.sshHost ?? context.tr('로컬')} · ${item.worktreePath}',
                        ),
                      ),
                  ],
                  if (value.review.pulledTasks.isNotEmpty) ...[
                    const SizedBox(height: AppSizes.p16),
                    Text(
                      context.tr('자동 Pull 결과'),
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                    for (final item in value.review.pulledTasks)
                      ListTile(
                        leading: Icon(
                          item.status == 'failed'
                              ? Icons.error_outline
                              : Icons.download_done,
                        ),
                        title: Text(item.title),
                        subtitle: Text(
                          '${item.branchName} · ${context.tr(item.status == 'failed' ? '실패' : '갱신됨')}\n${item.summary}',
                        ),
                      ),
                  ],
                  if (value.review.failures.isNotEmpty) ...[
                    const SizedBox(height: AppSizes.p16),
                    Text(
                      context.tr('동기화 실패'),
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                    for (final failure in value.review.failures)
                      ListTile(
                        leading: Icon(
                          Icons.warning_amber_outlined,
                          color: Theme.of(context).colorScheme.error,
                        ),
                        title: Text('${failure.operation} · ${failure.target}'),
                        subtitle: SelectableText(failure.reason),
                      ),
                  ],
                  if (value.review.mergedPullRequests.isNotEmpty) ...[
                    const SizedBox(height: AppSizes.p16),
                    Text(
                      context.tr('병합된 PR 태스크'),
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                    Text(
                      context.tr('삭제할 태스크만 선택하세요. 한 번에 최대 100개를 선택할 수 있습니다.'),
                    ),
                    for (final item in value.review.mergedPullRequests)
                      Semantics(
                        identifier: 'sync-review-task-${item.taskId}',
                        child: CheckboxListTile(
                          contentPadding: EdgeInsets.zero,
                          value: value.selectedTaskIds.contains(item.taskId),
                          onChanged:
                              value.isDeleting ||
                                  value.deletedTaskIds.contains(item.taskId)
                              ? null
                              : (_) => ref
                                    .read(provider.notifier)
                                    .toggle(item.taskId),
                          title: Text(item.title),
                          subtitle: Text(
                            '${item.branchName}\n${item.prUrl}${value.deletedTaskIds.contains(item.taskId) ? '\n${context.tr('삭제됨')}' : ''}',
                          ),
                        ),
                      ),
                    if (value.error != null)
                      Text(
                        context.tr(
                          value.error is DesktopRequestException
                              ? (value.error as DesktopRequestException)
                                        .message ??
                                    '태스크를 삭제하지 못했습니다.'
                              : '태스크를 삭제하지 못했습니다. 연결을 확인해 주세요.',
                        ),
                        style: TextStyle(
                          color: Theme.of(context).colorScheme.error,
                        ),
                      ),
                    if (value.lastDeletedCount > 0)
                      Text(
                        context.tr('{count}개 태스크를 삭제했습니다.', {
                          'count': value.lastDeletedCount,
                        }),
                      ),
                    if (isAnotherCleanupRunning)
                      Text(context.tr('다른 태스크를 정리하고 있습니다. 완료 후 다시 시도해 주세요.')),
                    const SizedBox(height: AppSizes.p16),
                    Semantics(
                      identifier: 'sync-review-delete',
                      child: OutlinedButton(
                        onPressed:
                            value.isDeleting ||
                                value.selectedTaskIds.isEmpty ||
                                isAnotherCleanupRunning
                            ? null
                            : () => _delete(
                                context,
                                ref,
                                value.selectedTaskIds.length,
                              ),
                        child: Text(
                          context.tr(value.isDeleting ? '삭제하는 중…' : '선택 항목 삭제'),
                        ),
                      ),
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
