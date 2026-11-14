import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_sizes.dart';
import '../../../common/l10n/app_localizations.dart';
import '../../../common/providers.dart';
import '../application/task_search_provider.dart';
import '../domain/board_task.dart';
import '../domain/board_view_options.dart';

class TaskQuickSearchScreen extends ConsumerStatefulWidget {
  const TaskQuickSearchScreen({super.key});
  @override
  ConsumerState<TaskQuickSearchScreen> createState() =>
      _TaskQuickSearchScreenState();
}

class _TaskQuickSearchScreenState extends ConsumerState<TaskQuickSearchScreen> {
  String _query = '';
  final _queryController = TextEditingController();
  @override
  void dispose() {
    _queryController.dispose();
    super.dispose();
  }

  String? _openingTaskId;
  String? _failure;

  Future<void> _open(BoardTask task) async {
    final connection = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    if (connection == null || _openingTaskId != null) return;
    setState(() {
      _openingTaskId = task.id;
      _failure = null;
    });
    try {
      final payload = await ref
          .read(desktopClientProvider)
          .fetchTask(connection, task.id);
      if (!mounted ||
          ref.read(connectionControllerProvider).value?.activeId !=
              connection.id) {
        return;
      }
      final current = payload == null
          ? null
          : BoardTask.fromJson(payload, const {});
      if (current == null) {
        setState(() {
          _failure = '태스크가 삭제되었거나 더 이상 존재하지 않습니다.';
          _openingTaskId = null;
        });
        return;
      }
      Navigator.of(context).pop(current);
    } catch (_) {
      if (mounted &&
          ref.read(connectionControllerProvider).value?.activeId ==
              connection.id) {
        setState(() {
          _openingTaskId = null;
          _failure = '태스크를 열지 못했습니다. 잠시 후 다시 시도해주세요.';
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final tasks = ref.watch(taskSearchProvider);
    ref.listen(
      connectionControllerProvider.select((value) => value.value?.activeId),
      (previous, next) {
        if (previous != next) {
          _query = '';
          _queryController.clear();
          _openingTaskId = null;
          _failure = null;
        }
      },
    );
    return Scaffold(
      appBar: AppBar(title: Text(context.tr('전체 태스크 검색'))),
      body: SafeArea(
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.all(AppSizes.p16),
              child: Semantics(
                identifier: 'task-search-query',
                child: TextField(
                  controller: _queryController,
                  autofocus: true,
                  decoration: InputDecoration(
                    labelText: context.tr('제목·프로젝트·브랜치 검색'),
                    prefixIcon: const Icon(Icons.search),
                  ),
                  onChanged: (value) => setState(() => _query = value),
                ),
              ),
            ),
            if (_failure != null)
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: AppSizes.p16),
                child: Text(
                  context.tr(_failure!),
                  style: TextStyle(color: Theme.of(context).colorScheme.error),
                ),
              ),
            Expanded(
              child: tasks.when(
                loading: () => const Center(child: CircularProgressIndicator()),
                error: (_, _) => Center(
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Text(context.tr('태스크 목록을 불러오지 못했습니다.')),
                      TextButton(
                        onPressed: () => ref.invalidate(taskSearchProvider),
                        child: Text(context.tr('다시 시도')),
                      ),
                    ],
                  ),
                ),
                data: (all) {
                  final scores = {
                    for (final task in all)
                      task.id: ?fuzzyTaskScore(task, _query),
                  };
                  final matches =
                      all.where((task) => scores.containsKey(task.id)).toList()
                        ..sort(
                          (a, b) => scores[a.id]!.compareTo(scores[b.id]!),
                        );
                  if (matches.isEmpty) {
                    return Center(child: Text(context.tr('검색 결과가 없습니다.')));
                  }
                  return ListView.builder(
                    itemCount: matches.length,
                    itemBuilder: (context, index) {
                      final task = matches[index];
                      return ListTile(
                        key: ValueKey('search-${task.id}'),
                        title: Text(task.title),
                        subtitle: Text(
                          [
                            task.projectName,
                            task.branchName,
                          ].whereType<String>().join(' · '),
                        ),
                        trailing: _openingTaskId == task.id
                            ? const SizedBox(
                                width: 24,
                                height: 24,
                                child: CircularProgressIndicator(),
                              )
                            : Text(task.status.label),
                        enabled: _openingTaskId == null,
                        onTap: () => _open(task),
                      );
                    },
                  );
                },
              ),
            ),
          ],
        ),
      ),
    );
  }
}
