class BackgroundSyncStatus {
  const BackgroundSyncStatus({
    required this.phase,
    required this.isRunning,
    required this.completedStages,
    required this.reviewNeeded,
    required this.boardUpdated,
    this.error,
  });

  const BackgroundSyncStatus.idle()
    : phase = 'idle',
      isRunning = false,
      completedStages = 0,
      reviewNeeded = false,
      boardUpdated = false,
      error = null;

  final String phase;
  final bool isRunning;
  final int completedStages;
  final bool reviewNeeded;
  final bool boardUpdated;
  final String? error;
  static const stageCount = 3;
}
