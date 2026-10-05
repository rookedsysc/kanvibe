import 'package:flutter/material.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_sizes.dart';
import '../../../../common/constants/app_theme.dart';
import '../../../board/domain/board_task.dart';

/// 태스크의 정보. 데스크탑 dock의 첫 항목이 여는 패널과 같은 것을 담는다.
///
/// 데스크탑은 화면 왼쪽에 겹쳐 열지만 폰에는 그만한 가로 여유가 없어 아래에서 올려 연다.
/// 담는 항목과 순서는 데스크탑 정보 패널을 따른다. 데스크탑이 함께 보여 주는 변경 파일 수는
/// 보드 응답에 없고 따로 받아 올 경로도 아직 없어서 빠져 있다.
class TaskInfoSheet extends StatelessWidget {
  const TaskInfoSheet({required this.task, super.key});

  final BoardTask task;

  @override
  Widget build(BuildContext context) {
    return SafeArea(
      /// 설명은 길이에 상한이 없다. 흐르게 두지 않으면 긴 설명을 단 태스크에서만 시트가 넘친다
      child: SingleChildScrollView(
        padding: const EdgeInsets.fromLTRB(
          AppSizes.p24,
          AppSizes.p12,
          AppSizes.p24,
          AppSizes.p24,
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const _SheetHandle(),
            const SizedBox(height: AppSizes.p16),
            Text(task.title, style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: AppSizes.p16),
            _InfoRow(label: '상태', value: task.status.label),
            if (task.projectName != null)
              _InfoRow(label: '프로젝트', value: task.projectName!),
            if (task.branchName != null)
              _InfoRow(label: '브랜치', value: task.branchName!, isMono: true),
            if (task.baseBranch != null)
              _InfoRow(label: '베이스 브랜치', value: task.baseBranch!, isMono: true),
            if (task.agentType != null)
              _InfoRow(label: '에이전트', value: task.agentType!),
            if (task.isRemote)
              _InfoRow(label: '원격 호스트', value: task.sshHost!, isMono: true),
            if (task.priority != null)
              _InfoRow(label: '우선순위', value: task.priority!.label),
            if (task.description != null) ...[
              const SizedBox(height: AppSizes.p16),
              Text('설명', style: Theme.of(context).textTheme.labelSmall),
              const SizedBox(height: AppSizes.p4),
              Text(
                task.description!,
                style: Theme.of(context).textTheme.bodyMedium,
              ),
            ],
          ],
        ),
      ),
    );
  }
}

/// 아래에서 올라온 시트라는 것과 내려서 닫을 수 있다는 것을 같이 알린다
class _SheetHandle extends StatelessWidget {
  const _SheetHandle();

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Container(
        width: 32,
        height: 4,
        decoration: BoxDecoration(
          color: AppColors.borderStrong,
          borderRadius: BorderRadius.circular(AppSizes.radiusSm),
        ),
      ),
    );
  }
}

/// 이름과 값 한 줄. 값이 식별자면 고정폭으로 둔다
class _InfoRow extends StatelessWidget {
  const _InfoRow({
    required this.label,
    required this.value,
    this.isMono = false,
  });

  final String label;
  final String value;
  final bool isMono;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: AppSizes.p8),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 96,
            child: Text(label, style: Theme.of(context).textTheme.labelSmall),
          ),
          Expanded(
            child: Text(
              value,
              style: isMono
                  ? monoStyle(fontSize: 12, color: AppColors.textPrimary)
                  : Theme.of(context).textTheme.bodyMedium,
            ),
          ),
        ],
      ),
    );
  }
}
