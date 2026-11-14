import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/l10n/app_localizations.dart';
import '../../application/running_ai_panes.dart';

class RunningAiBadge extends ConsumerWidget {
  const RunningAiBadge({
    required this.worktreePath,
    required this.onPressed,
    super.key,
  });
  final String? worktreePath;
  final VoidCallback onPressed;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    if (worktreePath == null) return const SizedBox.shrink();
    final panes =
        ref
            .watch(runningAiPanesProvider)
            .value
            ?.where((pane) => pane.worktreePath == worktreePath)
            .toList() ??
        [];
    if (panes.isEmpty) return const SizedBox.shrink();
    return TextButton.icon(
      onPressed: onPressed,
      icon: const Icon(Icons.smart_toy_outlined, size: 16),
      label: Text(context.tr('AI {count}개 실행 중', {'count': panes.length})),
      style: TextButton.styleFrom(minimumSize: const Size(48, 48)),
    );
  }
}
