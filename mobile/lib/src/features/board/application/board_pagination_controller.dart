import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/providers.dart';
import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../data/board_snapshot.dart';
import '../domain/board_task.dart';
import '../domain/task_status.dart';

final boardPaginationProvider =
    AsyncNotifierProvider<BoardPaginationController, BoardPaginationState>(
      BoardPaginationController.new,
      retry: (_, _) => null,
    );

class BoardPaginationState {
  const BoardPaginationState(
    this.snapshot, {
    this.isLoadingMore = false,
    this.loadMoreFailed = false,
  });
  final BoardSnapshot snapshot;
  final bool isLoadingMore;
  final bool loadMoreFailed;
}

class BoardPaginationController extends AsyncNotifier<BoardPaginationState> {
  int _generation = 0;
  DesktopConnection? _connection;

  @override
  Future<BoardPaginationState> build() async {
    _generation++;
    _connection = (await ref.watch(connectionControllerProvider.future))
        .activeConnection;
    return BoardPaginationState(await ref.watch(boardProvider.future));
  }

  Future<void> loadMore() async {
    final previous = state.value;
    final connection = _connection;
    final generation = _generation;
    if (previous == null ||
        previous.isLoadingMore ||
        !previous.snapshot.hasMoreDone ||
        connection == null) {
      return;
    }
    state = AsyncData(
      BoardPaginationState(previous.snapshot, isLoadingMore: true),
    );
    try {
      final payload = await ref
          .read(desktopClientProvider)
          .requestJson(
            connection,
            'GET',
            ['tasks', 'done'],
            query: {
              'offset': previous.snapshot
                  .tasksIn(TaskStatus.done)
                  .length
                  .toString(),
              'limit': '20',
            },
          );
      if (!ref.mounted || generation != _generation) return;
      final projectNames = {
        for (final p in previous.snapshot.projects) p.id: p.name,
      };
      final projectColors = {
        for (final p in previous.snapshot.projects)
          if (p.color != null) p.id: p.color!,
      };
      final projectIcons = {
        for (final p in previous.snapshot.projects)
          if (p.iconDataUrl != null) p.id: p.iconDataUrl!,
      };
      final done = {
        for (final t in previous.snapshot.tasksIn(TaskStatus.done)) t.id: t,
      };
      for (final raw in (payload['tasks'] as List)) {
        final task = BoardTask.fromJson(
          Map<String, dynamic>.from(raw as Map),
          projectNames,
          projectColorsById: projectColors,
          projectIconsById: projectIcons,
        );
        if (task != null) done[task.id] = task;
      }
      state = AsyncData(
        BoardPaginationState(
          previous.snapshot.withTasks({
            ...previous.snapshot.tasksByStatus,
            TaskStatus.done: done.values.toList(),
          }, doneTotal: payload['doneTotal'] as int),
        ),
      );
    } catch (error) {
      if (error is DesktopRequestException &&
          error.isUnauthorized &&
          ref.mounted) {
        await ref
            .read(connectionControllerProvider.notifier)
            .forgetLocally(connection.id, connection.token);
      }
      if (ref.mounted && generation == _generation) {
        state = AsyncData(
          BoardPaginationState(previous.snapshot, loadMoreFailed: true),
        );
      }
    }
  }
}
