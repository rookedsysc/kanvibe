class LiveAiSubtask {
  const LiveAiSubtask({required this.id, this.name, this.lastActiveAt});
  final String id;
  final String? name;
  final DateTime? lastActiveAt;
}

class LiveAiSession {
  const LiveAiSession({
    required this.provider,
    required this.state,
    required this.runningSubtasks,
    this.sessionId,
    this.currentTask,
    this.lastActiveAt,
    this.terminalWindowId,
    this.terminalWindowName,
  });
  final String provider;
  final String? sessionId;
  final String? currentTask;
  final String state;
  final DateTime? lastActiveAt;
  final List<LiveAiSubtask> runningSubtasks;
  final String? terminalWindowId;
  final String? terminalWindowName;
}

class LiveAiSessions {
  const LiveAiSessions({
    required this.taskId,
    required this.isRemote,
    required this.sessions,
  });
  final String taskId;
  final bool isRemote;
  final List<LiveAiSession> sessions;
}
