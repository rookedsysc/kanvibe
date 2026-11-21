import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../../board/domain/board_task.dart';
import '../data/task_details_api.dart';

final taskDetailsProvider = AsyncNotifierProvider.autoDispose
    .family<TaskDetailsController, BoardTask, String>(
      TaskDetailsController.new,
      retry: (_, _) => null,
    );

class TaskDetailsController extends AsyncNotifier<BoardTask> {
  TaskDetailsController(this.taskId);
  final String taskId;
  int _generation = 0;

  @override
  Future<BoardTask> build() async {
    ++_generation;
    final connection = (await ref.watch(connectionControllerProvider.future))
        .activeConnection;
    if (connection == null) throw StateError('연결된 PC가 없습니다.');
    return TaskDetailsApi(
      ref.read(desktopClientProvider),
      connection,
    ).read(taskId);
  }

  DesktopConnection _connection() {
    final connection = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    if (connection == null) throw StateError('연결된 PC가 없습니다.');
    return connection;
  }

  Future<BoardTask?> save({
    required String description,
    String? priority,
    DesktopConnection? expectedConnection,
  }) async {
    final connection = expectedConnection ?? _connection();
    if (!_isCurrent(connection)) return null;
    await future;
    if (!ref.mounted || !_isCurrent(connection)) return null;
    final generation = _generation;
    final task = await TaskDetailsApi(
      ref.read(desktopClientProvider),
      connection,
    ).update(taskId, description: description, priority: priority);
    if (!ref.mounted ||
        generation != _generation ||
        ref.read(connectionControllerProvider).value?.activeConnection !=
            connection) {
      return null;
    }
    state = AsyncData(task);
    ref.invalidate(boardProvider);
    return task;
  }

  bool _isCurrent(DesktopConnection connection) {
    final current = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    return current?.id == connection.id && current?.token == connection.token;
  }

  Future<bool> setProjectColor(
    String projectId,
    String color, {
    DesktopConnection? expectedConnection,
  }) async {
    final connection = expectedConnection ?? _connection();
    if (!_isCurrent(connection)) return false;
    final generation = _generation;
    await TaskDetailsApi(
      ref.read(desktopClientProvider),
      connection,
    ).setColor(projectId, color);
    if (ref.mounted && generation == _generation && _isCurrent(connection)) {
      ref.invalidate(boardProvider);
      ref.invalidateSelf();
      return true;
    }
    return false;
  }

  Future<bool> isDoneNoticeDismissed() => TaskDetailsApi(
    ref.read(desktopClientProvider),
    _connection(),
  ).isDoneNoticeDismissed();
  Future<void> dismissDoneNotice() => TaskDetailsApi(
    ref.read(desktopClientProvider),
    _connection(),
  ).dismissDoneNotice();

  Future<String?> findPullRequest() async {
    final connection = _connection();
    try {
      final url = await TaskDetailsApi(
        ref.read(desktopClientProvider),
        connection,
      ).findPullRequest(taskId);
      if (ref.mounted) {
        ref.invalidate(boardProvider);
        ref.invalidateSelf();
      }
      return url;
    } on DesktopRequestException catch (error) {
      if (error.isUnauthorized && ref.mounted) {
        await ref
            .read(connectionControllerProvider.notifier)
            .forgetLocally(connection.id, connection.token);
      }
      rethrow;
    }
  }
}

final relatedTasksProvider = FutureProvider.autoDispose
    .family<List<BoardTask>, String>((ref, projectId) async {
      final connection = (await ref.watch(connectionControllerProvider.future))
          .activeConnection;
      if (connection == null) throw StateError('연결된 PC가 없습니다.');
      return TaskDetailsApi(
        ref.read(desktopClientProvider),
        connection,
      ).related(projectId);
    }, retry: (_, _) => null);

final baseBranchTaskProvider = FutureProvider.autoDispose
    .family<BoardTask?, ({String projectId, String branch})>((
      ref,
      target,
    ) async {
      final connection = (await ref.watch(connectionControllerProvider.future))
          .activeConnection;
      if (connection == null) throw StateError('연결된 PC가 없습니다.');
      return TaskDetailsApi(
        ref.read(desktopClientProvider),
        connection,
      ).baseTask(target.projectId, target.branch);
    }, retry: (_, _) => null);
