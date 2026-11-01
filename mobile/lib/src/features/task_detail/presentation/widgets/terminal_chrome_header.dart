import 'package:flutter/material.dart';

import '../../../../common/constants/project_colors.dart';
import '../../../../common/constants/app_sizes.dart';
import '../../../board/domain/board_task.dart';

class TerminalChromeHeader extends StatelessWidget {
  const TerminalChromeHeader({required this.task, super.key});

  final BoardTask task;

  @override
  Widget build(BuildContext context) {
    final projectName = task.projectName;
    final branch = task.branchName ?? task.title;
    return Row(
      children: [
        if (projectName != null) ...[
          Flexible(
            flex: 2,
            child: Tooltip(
              message: projectName,
              child: Container(
                padding: const EdgeInsets.symmetric(
                  horizontal: AppSizes.p8,
                  vertical: AppSizes.p4,
                ),
                decoration: BoxDecoration(
                  color: projectColor(projectName, task.projectColor),
                  borderRadius: BorderRadius.circular(AppSizes.radiusSm),
                ),
                child: Text(
                  projectName,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: Theme.of(context).textTheme.labelSmall?.copyWith(
                    color: projectTextColor(
                      projectColor(projectName, task.projectColor),
                    ),
                  ),
                ),
              ),
            ),
          ),
          const SizedBox(width: AppSizes.p8),
        ],
        Expanded(
          flex: 3,
          child: Tooltip(
            message: branch,
            child: Text(
              branch,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(context).textTheme.titleMedium,
            ),
          ),
        ),
      ],
    );
  }
}
