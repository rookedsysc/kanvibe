class AiUsageWindow {
  const AiUsageWindow({
    required this.kind,
    required this.usedPercent,
    this.modelName,
    this.resetsAt,
  });
  final String kind;
  final String? modelName;
  final double usedPercent;
  final DateTime? resetsAt;
}

class AiUsageAccount {
  const AiUsageAccount({
    required this.accountId,
    required this.provider,
    required this.label,
    required this.status,
    required this.fetchedAt,
    required this.windows,
    this.planName,
    this.reason,
  });
  final String accountId;
  final String provider;
  final String label;
  final String status;
  final String? planName;
  final String? reason;
  final DateTime? fetchedAt;
  final List<AiUsageWindow> windows;
}

class AiUsageSnapshot {
  const AiUsageSnapshot({required this.accounts, required this.fetchedAt});
  final List<AiUsageAccount> accounts;
  final DateTime? fetchedAt;
}
