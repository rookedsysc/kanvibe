import 'package:flutter/material.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_sizes.dart';
import '../../../board/domain/board_task.dart';

/// 터미널 바로 위에 붙는 머리글.
///
/// 데스크탑 태스크 화면은 터미널 상자 안쪽 맨 위에 `프로젝트 | 태스크 제목`을 붙여 둔다.
/// 모바일은 태스크 제목을 이미 AppBar에 보여 주므로 같은 자리에 프로젝트 이름만 둔다.
/// 탭을 옮겨 다니다 보면 pane 이름만으로는 어느 프로젝트의 터미널인지 알 수 없기 때문이다.
class TerminalChromeHeader extends StatelessWidget {
  const TerminalChromeHeader({required this.task, super.key});

  final BoardTask task;

  @override
  Widget build(BuildContext context) {
    final projectName = task.projectName;
    if (projectName == null) return const SizedBox.shrink();

    return Container(
      constraints: const BoxConstraints(minHeight: 32),
      padding: const EdgeInsets.symmetric(
        horizontal: AppSizes.p12,
        vertical: AppSizes.p4,
      ),
      alignment: Alignment.centerLeft,
      decoration: const BoxDecoration(
        color: AppColors.terminalChrome,
        border: Border(bottom: BorderSide(color: AppColors.borderDefault)),
      ),
      child: Text(
        projectName,
        overflow: TextOverflow.ellipsis,
        style: Theme.of(context).textTheme.labelSmall,
      ),
    );
  }
}
