import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/l10n/app_localizations.dart';
import '../../board/domain/board_task.dart';
import '../application/task_details_controller.dart';

class RelatedTasksScreen extends ConsumerWidget {
  const RelatedTasksScreen({
    required this.projectId,
    this.baseBranch,
    super.key,
  });
  final String projectId;
  final String? baseBranch;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final provider = relatedTasksProvider(projectId);
    return Scaffold(
      appBar: AppBar(title: Text(context.tr('프로젝트 작업'))),
      body: ref
          .watch(provider)
          .when(
            loading: () => const Center(child: CircularProgressIndicator()),
            error: (_, _) => Center(
              child: TextButton(
                onPressed: () => ref.invalidate(provider),
                child: Text(context.tr('다시 시도')),
              ),
            ),
            data: (tasks) => ListView(
              children: [
                if (baseBranch != null)
                  ListTile(
                    leading: const Icon(Icons.account_tree_outlined),
                    title: Text(context.tr('기준 브랜치 작업')),
                    subtitle: Text(baseBranch!),
                    onTap: () async {
                      try {
                        final task = await ref.read(
                          baseBranchTaskProvider((
                            projectId: projectId,
                            branch: baseBranch!,
                          )).future,
                        );
                        if (!context.mounted) return;
                        if (task != null) {
                          Navigator.pop(context, task);
                        } else {
                          ScaffoldMessenger.of(context).showSnackBar(
                            SnackBar(
                              content: Text(context.tr('해당 브랜치의 태스크가 없습니다.')),
                            ),
                          );
                        }
                      } catch (_) {
                        if (context.mounted) {
                          ScaffoldMessenger.of(context).showSnackBar(
                            SnackBar(
                              content: Text(context.tr('작업을 불러오지 못했습니다.')),
                            ),
                          );
                        }
                      }
                    },
                  ),
                if (tasks.isEmpty)
                  Padding(
                    padding: const EdgeInsets.all(24),
                    child: Text(context.tr('표시할 태스크가 없습니다.')),
                  ),
                for (final task in tasks)
                  ListTile(
                    title: Text(task.title),
                    subtitle: Text(
                      '${task.status.label} · ${task.branchName ?? ''}',
                    ),
                    trailing: const Icon(Icons.chevron_right),
                    onTap: () => Navigator.pop<BoardTask>(context, task),
                  ),
              ],
            ),
          ),
    );
  }
}
