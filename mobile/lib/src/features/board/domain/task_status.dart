import 'package:flutter/widgets.dart';

import '../../../common/constants/app_colors.dart';

/// 보드의 상태 칸.
///
/// 값과 순서는 데스크탑의 `TaskStatus` / `TASK_STATUS_ORDER`를 그대로 따른다.
/// 진행 중 상태의 값은 `in_progress`가 아니라 `progress`다.
enum TaskStatus {
  todo('todo', 'Todo', AppColors.statusTodo),
  progress('progress', 'Progress', AppColors.statusProgress),
  pending('pending', 'Pending', AppColors.statusPending),
  review('review', 'Review', AppColors.statusReview),
  done('done', 'Done', AppColors.statusDone);

  const TaskStatus(this.wireValue, this.label, this.color);

  final String wireValue;
  final String label;
  final Color color;

  /// 데스크탑이 보낸 값에 대응하는 칸. 모르는 값은 없다고 보고 null을 준다
  static TaskStatus? fromWire(String value) {
    for (final status in TaskStatus.values) {
      if (status.wireValue == value) {
        return status;
      }
    }
    return null;
  }
}
