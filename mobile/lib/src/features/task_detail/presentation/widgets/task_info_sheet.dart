import '../../../diff/presentation/widgets/task_diff_summary.dart';

import 'package:flutter/material.dart';

import '../../../../common/l10n/app_localizations.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_sizes.dart';
import '../../../../common/constants/app_theme.dart';
import '../../../board/domain/board_task.dart';

/// 태스크의 정보. 데스크탑 dock의 첫 항목이 여는 패널과 같은 것을 담는다.
///
/// 데스크탑은 화면 왼쪽에 겹쳐 열지만 폰에는 그만한 가로 여유가 없어 아래에서 올려 연다.
/// 담는 항목과 순서는 데스크탑 정보 패널을 따른다. 변경량과 날짜를 함께 보여 준다.
/// 날짜는 현재 기기의 시간대와 언어로 표시한다.
class TaskInfoSheet extends StatelessWidget {
  const TaskInfoSheet({required this.task, super.key});

  final BoardTask task;

  String _date(BuildContext context, DateTime value) {
    final local = value.toLocal();
    final format = MaterialLocalizations.of(context);
    return "${format.formatShortDate(local)} ${format.formatTimeOfDay(TimeOfDay.fromDateTime(local), alwaysUse24HourFormat: MediaQuery.alwaysUse24HourFormatOf(context))}";
  }

  @override
  Widget build(BuildContext context) {
    return SafeArea(
      /// 설명은 길이에 상한이 없다. 흐르게 두지 않으면 긴 설명을 단 태스크에서만 시트가 넘친다
      child: SingleChildScrollView(
        padding: EdgeInsets.fromLTRB(
          AppSizes.p24,
          AppSizes.p12,
          AppSizes.p24,
          AppSizes.p24,
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            _SheetHandle(),
            SizedBox(height: AppSizes.p16),
            Text(task.title, style: Theme.of(context).textTheme.titleMedium),
            SizedBox(height: AppSizes.p16),
            if (task.branchName != null) TaskDiffSummary(taskId: task.id),
            _InfoRow(
              label: context.tr('상태'),
              value: context.tr(task.status.label),
            ),
            if (task.projectName != null)
              _InfoRow(label: context.tr('프로젝트'), value: task.projectName!),
            if (task.branchName != null)
              _InfoRow(
                label: context.tr('브랜치'),
                value: task.branchName!,
                isMono: true,
              ),
            if (task.baseBranch != null)
              _InfoRow(
                label: context.tr('베이스 브랜치'),
                value: task.baseBranch!,
                isMono: true,
              ),
            if (task.createdAt != null)
              _InfoRow(
                label: context.tr('생성 시각'),
                value: _date(context, task.createdAt!),
              ),
            if (task.updatedAt != null)
              _InfoRow(
                label: context.tr('수정 시각'),
                value: _date(context, task.updatedAt!),
              ),
            if (task.agentType != null)
              _InfoRow(label: context.tr('에이전트'), value: task.agentType!),
            if (task.isRemote)
              _InfoRow(
                label: context.tr('원격 호스트'),
                value: task.sshHost!,
                isMono: true,
              ),
            if (task.priority != null)
              _InfoRow(
                label: context.tr('우선순위'),
                value: context.tr(task.priority!.label),
              ),
            if (task.description != null) ...[
              SizedBox(height: AppSizes.p16),
              Text(
                context.tr('설명'),
                style: Theme.of(context).textTheme.labelSmall,
              ),
              SizedBox(height: AppSizes.p4),
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
          color: context.colors.borderStrong,
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
      padding: EdgeInsets.only(bottom: AppSizes.p8),
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
                  ? monoStyle(fontSize: 12, color: context.colors.textPrimary)
                  : Theme.of(context).textTheme.bodyMedium,
            ),
          ),
        ],
      ),
    );
  }
}
