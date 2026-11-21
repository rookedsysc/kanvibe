class MergedReviewTask {
  const MergedReviewTask({
    required this.taskId,
    required this.title,
    required this.branchName,
    required this.prUrl,
    required this.mergedAt,
  });
  final String taskId, title, branchName, prUrl;
  final DateTime? mergedAt;
}

class RegisteredReviewWorktree {
  const RegisteredReviewWorktree({
    required this.taskId,
    required this.projectName,
    required this.branchName,
    required this.worktreePath,
    this.sshHost,
  });
  final String taskId, projectName, branchName, worktreePath;
  final String? sshHost;
}

class PulledReviewTask {
  const PulledReviewTask({
    required this.taskId,
    required this.title,
    required this.branchName,
    required this.status,
    required this.summary,
    this.sshHost,
  });
  final String taskId, title, branchName, status, summary;
  final String? sshHost;
}

class SyncReviewFailure {
  const SyncReviewFailure({
    required this.operation,
    required this.target,
    required this.reason,
  });
  final String operation, target, reason;
}

class SyncReview {
  const SyncReview({
    required this.mergedPullRequests,
    required this.registeredWorktrees,
    required this.pulledTasks,
    required this.failures,
  });
  final List<MergedReviewTask> mergedPullRequests;
  final List<RegisteredReviewWorktree> registeredWorktrees;
  final List<PulledReviewTask> pulledTasks;
  final List<SyncReviewFailure> failures;
  bool get isEmpty =>
      mergedPullRequests.isEmpty &&
      registeredWorktrees.isEmpty &&
      pulledTasks.isEmpty &&
      failures.isEmpty;
}
