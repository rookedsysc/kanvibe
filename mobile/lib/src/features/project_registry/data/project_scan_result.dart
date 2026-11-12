class ProjectScanResult {
  const ProjectScanResult({
    this.registered = const [],
    this.skipped = const [],
    this.errors = const [],
    this.worktreeTasks = const [],
    this.registeredWorktrees = const [],
  });

  factory ProjectScanResult.fromJson(Map<String, dynamic> json) =>
      ProjectScanResult(
        registered: List<String>.from(json['registered'] as List),
        skipped: List<String>.from(json['skipped'] as List),
        errors: List<String>.from(json['errors'] as List),
        worktreeTasks: List<String>.from(json['worktreeTasks'] as List),
        registeredWorktrees: List<String>.from(
          json['registeredWorktrees'] as List,
        ),
      );

  final List<String> registered;
  final List<String> skipped;
  final List<String> errors;
  final List<String> worktreeTasks;
  final List<String> registeredWorktrees;

  bool get isEmpty =>
      registered.isEmpty &&
      skipped.isEmpty &&
      errors.isEmpty &&
      worktreeTasks.isEmpty &&
      registeredWorktrees.isEmpty;
}
