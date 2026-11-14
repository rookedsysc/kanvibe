import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_sizes.dart';
import '../../../common/l10n/app_localizations.dart';
import '../../../common/network/desktop_connection.dart';
import '../../board/domain/board_project.dart';
import '../application/registered_projects_controller.dart';

class RegisteredProjectsScreen extends ConsumerWidget {
  const RegisteredProjectsScreen({required this.connection, super.key});
  final DesktopConnection connection;
  Future<void> _remove(
    BuildContext context,
    WidgetRef ref,
    BoardProject project,
  ) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(context.tr('프로젝트 등록을 삭제할까요?')),
        content: Text(
          '${project.name}\n${context.tr('프로젝트와 연결된 태스크 등록을 삭제합니다. Git 저장소와 폴더는 보존됩니다.')}',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(context.tr('취소')),
          ),
          TextButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(context.tr('삭제')),
          ),
        ],
      ),
    );
    if (context.mounted && confirmed == true) {
      await ref
          .read(registeredProjectsProvider(connection).notifier)
          .remove(project.id);
    }
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final provider = registeredProjectsProvider(connection);
    final projects = ref.watch(provider);
    return Scaffold(
      appBar: AppBar(
        title: Text(context.tr('등록된 프로젝트')),
        actions: [
          IconButton(
            tooltip: context.tr('새로고침'),
            onPressed: () => ref.invalidate(provider),
            icon: const Icon(Icons.refresh),
          ),
        ],
      ),
      body: SafeArea(
        child: projects.when(
          loading: () => const Center(child: CircularProgressIndicator()),
          error: (_, _) => Center(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(context.tr('프로젝트 목록을 불러오지 못했습니다.')),
                TextButton(
                  onPressed: () => ref.invalidate(provider),
                  child: Text(context.tr('프로젝트 목록 다시 불러오기')),
                ),
              ],
            ),
          ),
          data: (value) => ListView(
            padding: const EdgeInsets.all(AppSizes.p16),
            children: [
              if (value.error != null)
                Text(
                  context.tr('프로젝트 등록을 삭제하지 못했습니다. 다시 시도해 주세요.'),
                  style: TextStyle(color: Theme.of(context).colorScheme.error),
                ),
              if (value.projects.isEmpty) Text(context.tr('등록된 프로젝트가 없습니다.')),
              for (final project in value.projects)
                Card(
                  child: ListTile(
                    title: Text(project.name),
                    subtitle: Text(
                      '${project.sshHost ?? context.tr('로컬')} · ${project.repoPath ?? ''}',
                    ),
                    trailing: Semantics(
                      identifier: 'project-delete-${project.id}',
                      child: IconButton(
                        tooltip: context.tr('프로젝트 등록 삭제'),
                        onPressed: value.deletingId != null
                            ? null
                            : () => _remove(context, ref, project),
                        icon: value.deletingId == project.id
                            ? const SizedBox(
                                width: AppSizes.p16,
                                height: AppSizes.p16,
                                child: CircularProgressIndicator(),
                              )
                            : const Icon(Icons.delete_outline),
                      ),
                    ),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }
}
