import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../data/sync_review_repository.dart';
import '../domain/sync_review.dart';
import 'notification_inbox_controller.dart';
import 'sync_review_cleanup_controller.dart';

final syncReviewRepositoryProvider = Provider<SyncReviewRepository>(
  (ref) => SyncReviewRepository(ref.watch(desktopClientProvider)),
);
final syncReviewProvider = AsyncNotifierProvider.autoDispose
    .family<SyncReviewController, SyncReviewState, String>(
      SyncReviewController.new,
      retry: (_, _) => null,
    );

final syncReviewNotificationIdProvider = Provider<String?>((ref) {
  final cleanup = ref.watch(syncReviewCleanupProvider);
  if (cleanup != null) return cleanup.notificationId;
  final inbox = ref.watch(notificationInboxProvider);
  if (inbox.isLoading || inbox.hasError) return null;
  final notifications =
      inbox.value?.notifications
          .where((entry) => entry.hasSyncReview)
          .toList() ??
      [];
  notifications.sort(
    (first, second) => second.createdAt.compareTo(first.createdAt),
  );
  return notifications.firstOrNull?.id;
});

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

  int get detectedCount =>
      review.mergedPullRequests
          .where((task) => !deletedTaskIds.contains(task.taskId))
          .length +
      review.registeredWorktrees.length +
      review.pulledTasks.length +
      review.failures.length;
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
    ref.watch(
      connectionControllerProvider.select(
        (value) => (
          value.value?.activeConnection?.id,
          value.value?.activeConnection?.token,
        ),
      ),
    );
    final connection = (await ref.read(connectionControllerProvider.future))
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
        connection == null ||
        !_current(connection, _generation)) {
      return false;
    }
    final generation = _generation;
    final cleanup = ref.read(syncReviewCleanupProvider.notifier);
    final job = cleanup.start(notificationId, connection);
    if (job == null) return false;
    ref.keepAlive();
    var deletedCount = 0;
    Object? deletionError;
    state = AsyncData(
      SyncReviewState(
        previous.review,
        selectedTaskIds: previous.selectedTaskIds,
        deletedTaskIds: previous.deletedTaskIds,
        isDeleting: true,
      ),
    );
    try {
      for (final taskId in previous.selectedTaskIds) {
        if (!_current(connection, generation)) return false;
        try {
          final deleted = await ref
              .read(syncReviewRepositoryProvider)
              .deleteTasks(connection, notificationId, {taskId});
          if (!_current(connection, generation)) return false;
          _deletedTaskIds = {..._deletedTaskIds, ...deleted};
          deletedCount += deleted.length;
        } catch (error) {
          if (!_current(connection, generation)) return false;
          deletionError ??= error;
          await _forget(error, connection);
          if (!_current(connection, generation)) return false;
        }
        state = AsyncData(
          SyncReviewState(
            previous.review,
            selectedTaskIds: Set.unmodifiable(
              previous.selectedTaskIds.difference(_deletedTaskIds),
            ),
            deletedTaskIds: Set.unmodifiable(_deletedTaskIds),
            isDeleting: true,
            error: deletionError,
            lastDeletedCount: deletedCount,
          ),
        );
      }
      if (!_current(connection, generation)) return false;
      state = AsyncData(
        SyncReviewState(
          previous.review,
          selectedTaskIds: Set.unmodifiable(
            previous.selectedTaskIds.difference(_deletedTaskIds),
          ),
          deletedTaskIds: Set.unmodifiable(_deletedTaskIds),
          error: deletionError,
          lastDeletedCount: deletedCount,
        ),
      );
      if (deletedCount > 0) ref.invalidate(boardProvider);
      return deletionError == null;
    } finally {
      if (ref.mounted) cleanup.finish(job);
    }
  }
}
