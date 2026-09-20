import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

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

/// 태스크를 다른 상태로 옮기는 시트.
///
/// 데스크탑 상태 패널에는 삭제와 hooks 설치 상태도 함께 있지만 여기에는 상태 이동만 둔다.
/// 삭제는 폰에서 되돌릴 방법이 없고, hooks는 데스크탑에서만 고칠 수 있는 것이라 안내할 것이 없다.
///
/// 옮기기에 성공하면 새 상태를 돌려주며 닫힌다. 실패하면 닫지 않고 사유를 남겨,
/// 사용자가 무엇이 안 됐는지 보고 다시 고를 수 있게 한다.
class TaskStatusSheet extends ConsumerStatefulWidget {
  const TaskStatusSheet({required this.task, super.key});

  final BoardTask task;

  @override
  ConsumerState<TaskStatusSheet> createState() => _TaskStatusSheetState();
}

class _TaskStatusSheetState extends ConsumerState<TaskStatusSheet> {
  bool _isMoving = false;
  String? _failureMessage;

  Future<void> _move(TaskStatus status) async {
    final connection = ref.read(connectionControllerProvider).value;
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
      Navigator.of(context).pop(status);
    }
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
                onPressed: _isMoving ? null : () => _move(status),
                style: OutlinedButton.styleFrom(
                  minimumSize: const Size.fromHeight(AppSizes.minTouchTarget),
                  foregroundColor: status.color,
                ),
                child: Text('${status.label}(으)로 옮기기'),
              ),
              const SizedBox(height: AppSizes.p8),
            ],
            if (_failureMessage != null) ...[
              const SizedBox(height: AppSizes.p8),
              Text(
                _failureMessage!,
                style: Theme.of(context).textTheme.bodySmall?.copyWith(
                  color: Theme.of(context).colorScheme.error,
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }
}
