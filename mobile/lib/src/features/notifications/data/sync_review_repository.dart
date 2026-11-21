import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../domain/sync_review.dart';

class SyncReviewRepository {
  const SyncReviewRepository(this.client);
  final DesktopClient client;
  Future<SyncReview> fetch(
    DesktopConnection connection,
    String notificationId,
  ) async {
    final review =
        (await client.requestJson(connection, 'GET', [
              'notifications',
              notificationId,
              'review',
            ]))['review']
            as Map;
    return SyncReview(
      mergedPullRequests: List.unmodifiable(
        (review['mergedPullRequests'] as List? ?? []).map((entry) {
          final raw = entry as Map;
          return MergedReviewTask(
            taskId: raw['taskId'] as String,
            title: raw['taskTitle'] as String,
            branchName: raw['branchName'] as String,
            prUrl: raw['prUrl'] as String,
            mergedAt: DateTime.tryParse(raw['mergedAt']?.toString() ?? ''),
          );
        }),
      ),
      registeredWorktrees: List.unmodifiable(
        (review['registeredWorktrees'] as List? ?? []).map((entry) {
          final raw = entry as Map;
          return RegisteredReviewWorktree(
            taskId: raw['taskId'] as String,
            projectName: raw['projectName'] as String,
            branchName: raw['branchName'] as String,
            worktreePath: raw['worktreePath'] as String,
            sshHost: raw['sshHost'] as String?,
          );
        }),
      ),
      pulledTasks: List.unmodifiable(
        (review['pulledTasks'] as List? ?? []).map((entry) {
          final raw = entry as Map;
          return PulledReviewTask(
            taskId: raw['taskId'] as String,
            title: raw['taskTitle'] as String,
            branchName: raw['branchName'] as String,
            status: raw['status'] as String,
            summary: raw['summary'] as String,
            sshHost: raw['sshHost'] as String?,
          );
        }),
      ),
      failures: List.unmodifiable(
        (review['failures'] as List? ?? []).map((entry) {
          final raw = entry as Map;
          return SyncReviewFailure(
            operation: raw['operation'] as String,
            target: raw['target'] as String,
            reason: raw['reason'] as String,
          );
        }),
      ),
    );
  }

  Future<List<String>> deleteTasks(
    DesktopConnection connection,
    String notificationId,
    Set<String> taskIds,
  ) async {
    final value = await client.requestJson(
      connection,
      'POST',
      ['notifications', notificationId, 'review', 'delete'],
      body: {'taskIds': taskIds.toList()},
      timeout: const Duration(minutes: 5),
    );
    return List<String>.unmodifiable(value['deletedTaskIds'] as List);
  }
}
