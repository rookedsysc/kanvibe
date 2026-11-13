import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../data/sync_review_repository.dart';
import '../domain/sync_review.dart';

final syncReviewRepositoryProvider = Provider<SyncReviewRepository>(
  (ref) => SyncReviewRepository(ref.watch(desktopClientProvider)),
);
final syncReviewProvider = AsyncNotifierProvider.autoDispose
    .family<SyncReviewController, SyncReviewState, String>(
      SyncReviewController.new,
      retry: (_, _) => null,
    );

class SyncReviewState {
  const SyncReviewState(
    this.review, {
    this.selectedTaskIds = const {},
    this.deletedTaskIds = const {},
    this.isDeleting = false,
    this.error,
    this.lastDeletedCount = 0,
  });
  final SyncReview review;
  final Set<String> selectedTaskIds, deletedTaskIds;
  final bool isDeleting;
  final Object? error;
  final int lastDeletedCount;
}

class SyncReviewController extends AsyncNotifier<SyncReviewState> {
  SyncReviewController(this.notificationId);
  final String notificationId;
  DesktopConnection? _connection;
  int _generation = 0;
  Set<String> _deletedTaskIds = {};
  @override
  Future<SyncReviewState> build() async {
    _generation++;
    final connection = (await ref.watch(connectionControllerProvider.future))
        .activeConnection;
    if (connection == null) throw StateError('연결된 PC가 없습니다.');
    if (_connection?.id != connection.id ||
        _connection?.token != connection.token) {
      _deletedTaskIds = {};
    }
    _connection = connection;
    try {
      return SyncReviewState(
        await ref
            .read(syncReviewRepositoryProvider)
            .fetch(connection, notificationId),
        deletedTaskIds: Set.unmodifiable(_deletedTaskIds),
      );
    } catch (error) {
      await _forget(error, connection);
      rethrow;
    }
  }

  bool _current(DesktopConnection connection, int generation) {
    final active = ref.mounted
        ? ref.read(connectionControllerProvider).value?.activeConnection
        : null;
    return ref.mounted &&
        generation == _generation &&
        active?.id == connection.id &&
        active?.token == connection.token;
  }

  Future<void> _forget(Object error, DesktopConnection connection) async {
    if (error is DesktopRequestException &&
        error.isUnauthorized &&
        _current(connection, _generation)) {
      await ref
          .read(connectionControllerProvider.notifier)
          .forgetLocally(connection.id, connection.token);
    }
  }

  void toggle(String taskId) {
    final current = state.value;
    if (current == null ||
        current.isDeleting ||
        current.deletedTaskIds.contains(taskId) ||
        !current.review.mergedPullRequests.any(
          (task) => task.taskId == taskId,
        )) {
      return;
    }
    final selected = {...current.selectedTaskIds};
    if (!selected.remove(taskId) && selected.length < 100) selected.add(taskId);
    state = AsyncData(
      SyncReviewState(
        current.review,
        selectedTaskIds: Set.unmodifiable(selected),
        deletedTaskIds: current.deletedTaskIds,
        lastDeletedCount: current.lastDeletedCount,
      ),
    );
  }

  Future<bool> deleteSelected() async {
    final previous = state.value;
    final connection = _connection;
    if (state.isLoading ||
        previous == null ||
        previous.isDeleting ||
        previous.selectedTaskIds.isEmpty ||
        connection == null) {
      return false;
    }
    final generation = _generation;
    state = AsyncData(
      SyncReviewState(
        previous.review,
        selectedTaskIds: previous.selectedTaskIds,
        deletedTaskIds: previous.deletedTaskIds,
        isDeleting: true,
      ),
    );
    try {
      final deleted = await ref
          .read(syncReviewRepositoryProvider)
          .deleteTasks(connection, notificationId, previous.selectedTaskIds);
      if (!_current(connection, generation)) return false;
      _deletedTaskIds = {..._deletedTaskIds, ...deleted};
      state = AsyncData(
        SyncReviewState(
          previous.review,
          selectedTaskIds: Set.unmodifiable(
            previous.selectedTaskIds.difference(deleted.toSet()),
          ),
          deletedTaskIds: Set.unmodifiable(_deletedTaskIds),
          lastDeletedCount: deleted.length,
        ),
      );
      ref.invalidate(boardProvider);
      return true;
    } catch (error) {
      if (!_current(connection, generation)) return false;
      state = AsyncData(
        SyncReviewState(
          previous.review,
          selectedTaskIds: previous.selectedTaskIds,
          deletedTaskIds: previous.deletedTaskIds,
          error: error,
        ),
      );
      await _forget(error, connection);
      return false;
    }
  }
}
