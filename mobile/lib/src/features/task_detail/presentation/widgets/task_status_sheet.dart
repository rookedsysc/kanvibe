import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_sizes.dart';
import '../../../../common/network/desktop_client.dart';
import '../../../../common/providers.dart';
import '../../../board/domain/board_task.dart';
import '../../../board/domain/task_status.dart';

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

/// 시트가 닫히며 남긴 결과. 상태를 옮겼거나 태스크를 지웠다
sealed class TaskStatusSheetResult {
  const TaskStatusSheetResult();
}

final class TaskStatusMoved extends TaskStatusSheetResult {
  const TaskStatusMoved(this.status);

  final TaskStatus status;
}

final class TaskDeleted extends TaskStatusSheetResult {
  const TaskDeleted({this.failedPeerCount = 0});

  /// 꺼져 있어 삭제를 전파받지 못해 아직 태스크가 남은 다른 데스크탑 수
  final int failedPeerCount;
}

/// 태스크를 다른 상태로 옮기거나 지우는 시트. 데스크탑 상태 패널처럼 상태 버튼 아래에 삭제를 따로 떼어 둔다.
///
/// hooks 설치 상태는 데스크탑에서만 고칠 수 있는 것이라 여기에는 두지 않는다.
/// 삭제는 되돌릴 수 없으므로 한 번 더 묻고, 커밋하지 않은 변경이 있어 데스크탑이 멈추면 데스크탑처럼 강제 삭제를 고르게 한다.
///
/// 성공하면 결과를 돌려주며 닫힌다. 실패하면 닫지 않고 사유를 남겨,
/// 사용자가 무엇이 안 됐는지 보고 다시 고를 수 있게 한다.
class TaskStatusSheet extends ConsumerStatefulWidget {
  const TaskStatusSheet({required this.task, super.key});

  final BoardTask task;

  @override
  ConsumerState<TaskStatusSheet> createState() => _TaskStatusSheetState();
}

class _TaskStatusSheetState extends ConsumerState<TaskStatusSheet> {
  bool _isMoving = false;
  bool _isDeleting = false;
  String? _failureMessage;

  bool get _isBusy => _isMoving || _isDeleting;

  Future<void> _move(TaskStatus status) async {
    final connection = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    if (connection == null) {
      return;
    }

    setState(() {
      _isMoving = true;
      _failureMessage = null;
    });

    try {
      await ref
          .read(desktopClientProvider)
          .updateTaskStatus(connection, widget.task.id, status);
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _isMoving = false;
        _failureMessage = _moveFailureMessage(error);
      });
      return;
    }

    /// 보드에도 같은 변화가 반영돼야 돌아갔을 때 옛 칸에 남아 있지 않다
    ref.invalidate(boardProvider);

    if (mounted) {
      Navigator.of(context).pop(TaskStatusMoved(status));
    }
  }

  Future<void> _confirmAndDelete() async {
    final isConfirmed = await _askConfirmation(
      title: '이 작업을 삭제하시겠습니까?',
      message: 'worktree·브랜치·세션이 함께 지워지고, 같은 태스크를 가진 다른 데스크탑에서도 삭제됩니다.',
      confirmLabel: '삭제',
    );
    if (isConfirmed) {
      await _delete(isForced: false);
    }
  }

  Future<void> _delete({required bool isForced}) async {
    final connection = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    if (connection == null) {
      return;
    }

    setState(() {
      _isDeleting = true;
      _failureMessage = null;
    });

    final int failedPeerCount;
    try {
      failedPeerCount = await ref
          .read(desktopClientProvider)
          .deleteTask(connection, widget.task.id, isForced: isForced);
    } on DesktopRequestException catch (error) {
      if (!mounted) {
        return;
      }
      setState(() => _isDeleting = false);
      if (error.statusCode == 409 && !isForced) {
        await _confirmForcedDelete();
        return;
      }

      // 이미 지워진 태스크는 지우려던 결과와 같으므로 보드로 돌아간다
      if (error.statusCode == 404) {
        _finishDeletion(failedPeerCount: 0);
        return;
      }
      setState(() => _failureMessage = error.message ?? '태스크를 삭제하지 못했습니다.');
      return;
    } catch (_) {
      if (!mounted) {
        return;
      }
      setState(() {
        _isDeleting = false;
        _failureMessage = '데스크탑에 닿지 못했습니다. 잠시 뒤 다시 시도해 주세요.';
      });
      return;
    }

    _finishDeletion(failedPeerCount: failedPeerCount);
  }

  Future<void> _confirmForcedDelete() async {
    final isConfirmed = await _askConfirmation(
      title: '커밋하지 않은 변경이 있습니다',
      message: '강제로 삭제하면 이 worktree의 커밋하지 않은 변경도 함께 사라집니다.',
      confirmLabel: '강제 삭제',
    );
    if (isConfirmed) {
      await _delete(isForced: true);
    }
  }

  void _finishDeletion({required int failedPeerCount}) {
    // 보드에서도 같은 태스크가 사라져야 돌아갔을 때 지운 칸이 남아 있지 않다
    ref.invalidate(boardProvider);
    if (mounted) {
      Navigator.of(context).pop(TaskDeleted(failedPeerCount: failedPeerCount));
    }
  }

  Future<bool> _askConfirmation({
    required String title,
    required String message,
    required String confirmLabel,
  }) async {
    final isConfirmed = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        backgroundColor: AppColors.bgSurface,
        title: Text(title),
        content: Text(message),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(dialogContext).pop(false),
            child: const Text('취소'),
          ),
          TextButton(
            onPressed: () => Navigator.of(dialogContext).pop(true),
            style: TextButton.styleFrom(foregroundColor: AppColors.statusError),
            child: Text(confirmLabel),
          ),
        ],
      ),
    );
    return isConfirmed ?? false;
  }

  String _moveFailureMessage(Object error) {
    if (error is! DesktopRequestException) {
      return '데스크탑에 닿지 못했습니다. 잠시 뒤 다시 시도해 주세요.';
    }
    if (error.statusCode == 404) {
      return '데스크탑에서 이미 지워진 태스크입니다.';
    }
    return error.message ?? '상태를 옮기지 못했습니다.';
  }

  @override
  Widget build(BuildContext context) {
    final targets = _movableStatuses
        .where((status) => status != widget.task.status)
        .toList();

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
                onPressed: _isBusy ? null : () => _move(status),
                style: OutlinedButton.styleFrom(
                  minimumSize: const Size.fromHeight(AppSizes.minTouchTarget),
                  foregroundColor: status.color,
                ),
                child: Text('${status.label}(으)로 옮기기'),
              ),
              const SizedBox(height: AppSizes.p8),
            ],
            const SizedBox(height: AppSizes.p8),
            const Divider(color: AppColors.borderDefault, height: 1),
            const SizedBox(height: AppSizes.p16),
            TextButton(
              onPressed: _isBusy ? null : _confirmAndDelete,
              style: TextButton.styleFrom(
                minimumSize: const Size.fromHeight(AppSizes.minTouchTarget),
                foregroundColor: AppColors.statusError,
                backgroundColor: AppColors.statusError.withValues(alpha: 0.12),
              ),
              child: Text(_isDeleting ? '삭제하는 중…' : '삭제'),
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
