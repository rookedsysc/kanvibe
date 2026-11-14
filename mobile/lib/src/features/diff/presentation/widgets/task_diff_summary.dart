import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_sizes.dart';
import '../../../../common/l10n/app_localizations.dart';
import '../../application/diff_controller.dart';
import '../diff_screen.dart';

class TaskDiffSummary extends ConsumerWidget {
  const TaskDiffSummary({
    required this.taskId,
    this.compact = false,
    super.key,
  });
  final String taskId;
  final bool compact;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final buttonStyle = compact
        ? TextButton.styleFrom(
            alignment: Alignment.centerLeft,
            minimumSize: const Size(AppSizes.minTouchTarget, AppSizes.p32),
            padding: EdgeInsets.zero,
            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          )
        : null;
    return ref
        .watch(diffStatsProvider)
        .when(
          loading: () => const SizedBox.shrink(),
          error: (_, _) => TextButton.icon(
            style: buttonStyle,
            onPressed: () => ref.invalidate(diffStatsProvider),
            icon: compact ? null : const Icon(Icons.refresh, size: 16),
            label: Text(context.tr('변경량 다시 조회')),
          ),
          data: (stats) {
            final task = stats[taskId];
            if (task == null) {
              return TextButton.icon(
                style: buttonStyle,
                onPressed: () => Navigator.of(context).push(
                  MaterialPageRoute<void>(
                    builder: (_) => DiffScreen(taskId: taskId),
                  ),
                ),
                icon: compact ? null : const Icon(Icons.difference_outlined),
                label: Text(context.tr('변경 검토')),
              );
            }
            return TextButton.icon(
              style: buttonStyle,
              onPressed: () => Navigator.of(context).push(
                MaterialPageRoute<void>(
                  builder: (_) => DiffScreen(taskId: taskId),
                ),
              ),
              icon: compact
                  ? null
                  : const Icon(Icons.difference_outlined, size: 18),
              label: Text.rich(
                TextSpan(
                  text: '${task.fileCount} ${context.tr('파일')}  ',
                  children: [
                    TextSpan(
                      text: '+${task.additions}',
                      style: TextStyle(
                        color: ColorScheme.fromSeed(
                          seedColor: AppColors.statusDone,
                          brightness: Theme.of(context).brightness,
                        ).primary,
                      ),
                    ),
                    TextSpan(
                      text: ' −${task.deletions}',
                      style: TextStyle(
                        color: ColorScheme.fromSeed(
                          seedColor: AppColors.statusError,
                          brightness: Theme.of(context).brightness,
                        ).primary,
                      ),
                    ),
                  ],
                ),
                style: TextStyle(
                  color: context.colors.textMuted,
                  fontSize: compact ? 11 : null,
                ),
              ),
            );
          },
        );
  }
}
