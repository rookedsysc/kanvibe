import '../../task_create/domain/session_type.dart';

class PcSettings {
  const PcSettings({
    required this.defaultSessionType,
    required this.isSyncEnabled,
    required this.intervalMinutes,
  });
  final SessionType defaultSessionType;
  final bool isSyncEnabled;
  final int intervalMinutes;
}

class SessionChangeTargets {
  const SessionChangeTargets({
    required this.taskCount,
    required this.liveSessionCount,
    required this.sshHosts,
  });
  final int taskCount;
  final int liveSessionCount;
  final List<String?> sshHosts;
}

sealed class PcOperationResult {
  const PcOperationResult();
}

class BulkSessionChangeResult extends PcOperationResult {
  const BulkSessionChangeResult({
    required this.changedCount,
    required this.failedCount,
  });
  final int changedCount;
  final int failedCount;
}

class PcSyncRunResult extends PcOperationResult {
  const PcSyncRunResult({
    required this.reviewNeeded,
    required this.boardUpdated,
  });
  final bool reviewNeeded;
  final bool boardUpdated;
}
