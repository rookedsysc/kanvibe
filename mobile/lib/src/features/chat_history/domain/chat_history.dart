class ChatSession {
  const ChatSession({
    required this.id,
    required this.provider,
    required this.title,
    this.updatedAt,
  });
  final String id;
  final String provider;
  final String title;
  final DateTime? updatedAt;
  String get key => '$provider:$id';

  factory ChatSession.fromJson(Map<String, dynamic> value) => ChatSession(
    id: value['id'] as String,
    provider: value['provider'] as String,
    title:
        value['title'] as String? ??
        value['firstUserPrompt'] as String? ??
        value['id'] as String,
    updatedAt: DateTime.tryParse(value['updatedAt'] as String? ?? ''),
  );
}

class ChatMessage {
  const ChatMessage({required this.role, required this.text, this.timestamp});
  final String role;
  final String text;
  final DateTime? timestamp;

  factory ChatMessage.fromJson(Map<String, dynamic> value) => ChatMessage(
    role: value['role'] as String,
    text: value['fullText'] as String? ?? value['text'] as String? ?? '',
    timestamp: DateTime.tryParse(value['timestamp'] as String? ?? ''),
  );
}

class ChatHistoryPage {
  const ChatHistoryPage({
    required this.sessions,
    required this.providers,
    this.nextCursor,
  });
  final List<ChatSession> sessions;
  final List<String> providers;
  final String? nextCursor;

  factory ChatHistoryPage.fromJson(Map<String, dynamic> value) =>
      ChatHistoryPage(
        sessions: (value['sessions'] as List)
            .map((entry) => ChatSession.fromJson(entry as Map<String, dynamic>))
            .toList(),
        providers: (value['sources'] as List)
            .where((entry) => entry['available'] == true)
            .map((entry) => entry['provider'] as String)
            .toList(),
        nextCursor: value['nextCursor'] as String?,
      );
}

class ChatMessagePage {
  const ChatMessagePage({required this.messages, this.nextCursor});
  final List<ChatMessage> messages;
  final String? nextCursor;

  factory ChatMessagePage.fromJson(Map<String, dynamic> value) =>
      ChatMessagePage(
        messages: (value['messages'] as List)
            .map((entry) => ChatMessage.fromJson(entry as Map<String, dynamic>))
            .toList(),
        nextCursor: value['nextCursor'] as String?,
      );
}
