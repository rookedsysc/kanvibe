import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_sizes.dart';
import '../../../board/application/task_deletion.dart';
import '../../../board/domain/board_task.dart';
import '../../../board/domain/task_status.dart';
import '../../../board/presentation/widgets/task_delete_dialogs.dart';
import '../../application/task_mutation.dart';
import 'busy_button_label.dart';

/// 옮겨 갈 수 있는 상태. 데스크탑 상태 패널이 내놓는 네 가지와 같다.
///
/// Pending이 빠진 것은 데스크탑도 그렇기 때문이다. 그 상태는 사람이 고르는 것이 아니라
/// 에이전트가 답을 기다릴 때 데스크탑이 스스로 옮겨 놓는 자리다.
const _movableStatuses = [
  TaskStatus.todo,
  TaskStatus.progress,
  TaskStatus.review,
  TaskStatus.done,
];

/// 태스크를 다른 상태로 옮기거나 지우는 시트. 데스크탑 상태 패널처럼 상태 버튼 아래에 삭제를 따로 떼어 둔다.
///
/// hooks 설치 상태는 데스크탑에서만 고칠 수 있는 것이라 여기에는 두지 않는다.
/// 삭제는 되돌릴 수 없으므로 한 번 더 묻고, 커밋하지 않은 변경이 있어 데스크탑이 멈추면 데스크탑처럼 강제 삭제를 고르게 한다.
///
/// 옮기기와 삭제는 시트를 먼저 닫아도 끝나도록 `taskMutationProvider`와 `taskDeletionProvider`가 들고 있다.
/// 옮기기에 성공하면 시트가 닫히고, 삭제에 성공하면 상세 화면이 시트와 함께 닫는다. 실패하면 닫지 않고 사유를 남겨,
/// 사용자가 무엇이 안 됐는지 보고 다시 고를 수 있게 한다.
class TaskStatusSheet extends ConsumerStatefulWidget {
  const TaskStatusSheet({required this.task, super.key});

  final BoardTask task;

  @override
  ConsumerState<TaskStatusSheet> createState() => _TaskStatusSheetState();
}

class _TaskStatusSheetState extends ConsumerState<TaskStatusSheet> {
  String? _failureMessage;

  Future<void> _move(TaskStatus status) async {
    setState(() => _failureMessage = null);
    final outcome = await ref
        .read(taskMutationProvider.notifier)
        .moveStatus(widget.task.id, status);
    if (!mounted) {
      return;
    }
    switch (outcome) {
      case TaskMutationSucceeded():
        Navigator.of(context).pop();
      case TaskMutationFailed(:final message):
        setState(() => _failureMessage = message);
      case TaskMutationSkipped():
        break;
    }
  }

  Future<void> _confirmAndDelete() async {
    final isConfirmed = await confirmTaskDeletion(context);
    if (isConfirmed) {
      await _delete(isForced: false);
    }
  }

  Future<void> _delete({
    required bool isForced,
    bool isRegistrationOnly = false,
  }) async {
    setState(() => _failureMessage = null);

    final outcome = await ref
        .read(taskDeletionProvider.notifier)
        .delete(
          widget.task.id,
          isForced: isForced,
          isRegistrationOnly: isRegistrationOnly,
        );
    if (!mounted) {
      return;
    }

    switch (outcome) {
      // 성공과 중복 요청은 controller와 상세 화면이 마무리한다
      case TaskDeletionSucceeded() || null:
        return;
      case TaskDeletionNeedsForce():
        await _confirmForcedDelete();
      case TaskDeletionNeedsRegistrationRemoval():
        final isConfirmed = await confirmTaskRegistrationRemoval(context);
        if (isConfirmed && mounted) {
          await _delete(isForced: false, isRegistrationOnly: true);
        }
      case TaskDeletionFailed(:final message):
        setState(() => _failureMessage = message);
    }
  }

  Future<void> _confirmForcedDelete() async {
    final isConfirmed = await confirmForcedTaskDeletion(context);
    if (isConfirmed) {
      await _delete(isForced: true);
    }
  }

  @override
  Widget build(BuildContext context) {
    final targets = _movableStatuses
        .where((status) => status != widget.task.status)
        .toList();
    final isDeleting = ref.watch(
      taskDeletionProvider.select(
        (progress) => progress[widget.task.id] is TaskDeletionRunning,
      ),
    );
    final movingStatus = ref.watch(
      taskMutationProvider.select((progress) {
        final taskProgress = progress[widget.task.id];
        return taskProgress is TaskStatusMoving ? taskProgress.status : null;
      }),
    );
    final isBusy = movingStatus != null || isDeleting;

    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(
          AppSizes.p24,
          AppSizes.p16,
          AppSizes.p24,
          AppSizes.p24,
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text('상태 옮기기', style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: AppSizes.p16),
            for (final status in targets) ...[
              OutlinedButton(
                onPressed: isBusy ? null : () => _move(status),
                style: OutlinedButton.styleFrom(
                  minimumSize: const Size.fromHeight(AppSizes.minTouchTarget),
                  foregroundColor: status.color,
                ),
                child: BusyButtonLabel(
                  label: '${status.label}(으)로 옮기기',
                  busyLabel: '${status.label}(으)로 옮기는 중…',
                  isBusy: movingStatus == status,
                ),
              ),
              const SizedBox(height: AppSizes.p8),
            ],
            const SizedBox(height: AppSizes.p8),
            const Divider(color: AppColors.borderDefault, height: 1),
            const SizedBox(height: AppSizes.p16),
            TextButton(
              onPressed: isBusy ? null : _confirmAndDelete,
              style: TextButton.styleFrom(
                minimumSize: const Size.fromHeight(AppSizes.minTouchTarget),
                foregroundColor: AppColors.statusError,
                backgroundColor: AppColors.statusError.withValues(alpha: 0.12),
              ),
              child: Text(isDeleting ? '삭제하는 중…' : '삭제'),
            ),
            if (_failureMessage != null) ...[
              const SizedBox(height: AppSizes.p8),
              Text(
                _failureMessage!,
                style: Theme.of(context).textTheme.bodySmall
                    ?.copyWith(color: Theme.of(context).colorScheme.error),
              ),
            ],
          ],
        ),
      ),
    );
  }
}
