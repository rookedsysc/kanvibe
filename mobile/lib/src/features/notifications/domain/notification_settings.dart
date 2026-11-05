class NotificationSettings {
  NotificationSettings({
    required this.isEnabled,
    required Iterable<String> enabledStatuses,
    this.isSaving = false,
  }) : enabledStatuses = Set.unmodifiable(enabledStatuses);

  final bool isEnabled;
  final Set<String> enabledStatuses;
  final bool isSaving;
}
