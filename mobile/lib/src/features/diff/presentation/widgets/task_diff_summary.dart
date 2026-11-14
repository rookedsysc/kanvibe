import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

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
  Widget build(BuildContext context, WidgetRef ref) => ref
      .watch(diffStatsProvider)
      .when(
        loading: () => const SizedBox.shrink(),
        error: (_, _) => TextButton.icon(
          onPressed: () => ref.invalidate(diffStatsProvider),
          icon: const Icon(Icons.refresh, size: 16),
          label: Text(context.tr('변경량 다시 조회')),
        ),
        data: (stats) {
          final task = stats[taskId];
          if (task == null) {
            return TextButton.icon(
              onPressed: () => Navigator.of(context).push(
                MaterialPageRoute<void>(
                  builder: (_) => DiffScreen(taskId: taskId),
                ),
              ),
              icon: const Icon(Icons.difference_outlined),
              label: Text(context.tr('변경 검토')),
            );
          }
          return TextButton.icon(
            onPressed: () => Navigator.of(context).push(
              MaterialPageRoute<void>(
                builder: (_) => DiffScreen(taskId: taskId),
              ),
            ),
            icon: const Icon(Icons.difference_outlined, size: 18),
            label: Text(
              '${task.fileCount} ${context.tr('파일')}  +${task.additions} −${task.deletions}',
              style: compact ? const TextStyle(fontSize: 11) : null,
            ),
          );
        },
      );
}
