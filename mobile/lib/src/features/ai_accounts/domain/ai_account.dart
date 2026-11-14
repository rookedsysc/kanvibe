enum AiAccountProvider {
  claude('Claude'),
  codex('Codex'),
  gemini('Gemini');

  const AiAccountProvider(this.label);
  final String label;
}

class AiAccount {
  const AiAccount({
    required this.accountId,
    required this.provider,
    required this.label,
    required this.isLoggedIn,
    required this.isRemovable,
    this.accountName,
    this.planName,
  });
  final String accountId;
  final AiAccountProvider provider;
  final String? accountName;
  final String label;
  final bool isLoggedIn;
  final bool isRemovable;
  final String? planName;
}
