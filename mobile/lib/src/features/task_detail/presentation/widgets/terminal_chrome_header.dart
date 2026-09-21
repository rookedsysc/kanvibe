import 'package:flutter/material.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_sizes.dart';
import '../../../../common/constants/app_theme.dart';
import '../../../board/domain/board_task.dart';

/// 터미널 바로 위에 붙는 머리글.
///
/// 데스크탑 태스크 화면은 터미널 상자 안쪽 맨 위에 `프로젝트 | 태스크 제목`을 붙여 둔다.
/// 모바일도 같은 자리에 같은 순서로 둔다. 탭을 옮겨 다니다 보면 pane 이름만으로는
/// 지금 어느 태스크의 터미널을 보고 있는지 알 수 없기 때문이다.
class TerminalChromeHeader extends StatelessWidget {
  const TerminalChromeHeader({required this.task, super.key});

  final BoardTask task;

  @override
  Widget build(BuildContext context) {
    return Container(
      constraints: const BoxConstraints(minHeight: 32),
      padding: const EdgeInsets.symmetric(
        horizontal: AppSizes.p12,
        vertical: AppSizes.p4,
      ),
      decoration: const BoxDecoration(
        color: AppColors.terminalChrome,
        border: Border(bottom: BorderSide(color: AppColors.borderDefault)),
      ),
      child: Row(
        children: [
          if (task.projectName != null) ...[
            Flexible(
              child: Text(
                task.projectName!,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.labelSmall,
              ),
            ),
            const SizedBox(width: AppSizes.p8),
            Text(
              '|',
              style: monoStyle(fontSize: 11, color: AppColors.borderStrong),
            ),
            const SizedBox(width: AppSizes.p8),
          ],
          Expanded(
            flex: 2,
            child: Text(
              task.title,
              overflow: TextOverflow.ellipsis,
              style: monoStyle(fontSize: 12, color: AppColors.terminalText),
            ),
          ),
        ],
      ),
    );
  }
}
