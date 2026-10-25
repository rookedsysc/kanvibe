import 'package:flutter/material.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/project_colors.dart';
import '../../../../common/constants/app_sizes.dart';
import '../../../board/domain/board_task.dart';

/// 터미널 바로 위에 붙는 머리글.
///
/// 데스크탑 태스크 화면은 터미널 상자 안쪽 맨 위에 `프로젝트 | 태스크 제목`을 붙여 둔다.
/// 모바일은 태스크 제목을 이미 AppBar에 보여 주므로 프로젝트 이름 옆에 새 태스크 동작을 둔다.
/// 탭을 옮겨 다니다 보면 pane 이름만으로는 어느 프로젝트의 터미널인지 알 수 없기 때문이다.
class TerminalChromeHeader extends StatelessWidget {
  const TerminalChromeHeader({
    required this.task,
    required this.onCreateTask,
    super.key,
  });

  final BoardTask task;
  final VoidCallback? onCreateTask;

  @override
  Widget build(BuildContext context) {
    final projectName = task.projectName;
    final background = projectName == null
        ? AppColors.terminalChrome
        : projectColor(projectName, task.projectColor);
    final foreground = projectName == null
        ? AppColors.textPrimary
        : projectTextColor(background);
    return Container(
      constraints: const BoxConstraints(minHeight: AppSizes.minTouchTarget),
      padding: const EdgeInsets.symmetric(horizontal: AppSizes.p12),
      alignment: Alignment.centerLeft,
      color: background,
      foregroundDecoration: const BoxDecoration(
        border: Border(bottom: BorderSide(color: AppColors.borderDefault)),
      ),
      child: Row(
        children: [
          Expanded(
            child: Text(
              projectName ?? '',
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(context).textTheme.labelSmall
                  ?.copyWith(color: foreground),
            ),
          ),
          const SizedBox(width: AppSizes.p8),
          Semantics(
            identifier: 'create-task-from-current',
            child: TextButton.icon(
              onPressed: onCreateTask,
              style: TextButton.styleFrom(
                minimumSize: const Size(
                  AppSizes.minTouchTarget,
                  AppSizes.minTouchTarget,
                ),
                padding: const EdgeInsets.symmetric(horizontal: AppSizes.p8),
              ),
              icon: const Icon(Icons.add),
              label: Text(onCreateTask == null ? '불러오는 중…' : '새 태스크'),
            ),
          ),
        ],
      ),
    );
  }
}
