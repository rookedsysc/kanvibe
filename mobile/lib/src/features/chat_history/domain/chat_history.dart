class ChatSession {
  const ChatSession({
    required this.id,
    required this.provider,
    required this.title,
    this.updatedAt,
    this.preview,
    this.messageCount = 0,
  });
  final String id;
  final String provider;
  final String title;
  final DateTime? updatedAt;
  final String? preview;
  final int messageCount;
  String get key => '$provider:$id';

  factory ChatSession.fromJson(Map<String, dynamic> value) => ChatSession(
    id: value['id'] as String,
    provider: value['provider'] as String,
    title:
        value['title'] as String? ??
        value['firstUserPrompt'] as String? ??
        value['id'] as String,
    updatedAt: DateTime.tryParse(value['updatedAt'] as String? ?? ''),
    preview: value['firstUserPrompt'] as String?,
    messageCount: value['messageCount'] as int? ?? 0,
  );
}

class ChatMessage {
  const ChatMessage({
    required this.role,
    required this.text,
    this.timestamp,
    this.attachments = const [],
    this.toolName,
    this.files = const [],
  });
  final String role;
  final String text;
  final DateTime? timestamp;
  final List<ChatAttachment> attachments;

  /// 도구 실행 메시지에서 AI가 부른 도구 이름. 오래된 데스크탑은 보내지 않는다
  final String? toolName;

  /// 이 메시지에서 AI가 만들거나 고친 파일
  final List<ChatMessageFile> files;

  factory ChatMessage.fromJson(Map<String, dynamic> value) => ChatMessage(
    role: value['role'] as String,
    text: value['fullText'] as String? ?? value['text'] as String? ?? '',
    timestamp: DateTime.tryParse(value['timestamp'] as String? ?? ''),
    attachments: (value['attachments'] as List? ?? [])
        .map((entry) => ChatAttachment.fromJson(entry as Map<String, dynamic>))
        .toList(),
    toolName: value['toolName'] is String ? value['toolName'] as String : null,
    files: [
      if (value['files'] case final List<dynamic> entries)
        for (final entry in entries) ?ChatMessageFile.tryParse(entry),
    ],
  );
}

enum ChatMessageFileChange { created, modified }

class ChatMessageFile {
  const ChatMessageFile({required this.path, required this.change});

  /// AI가 기록한 경로. 대개 PC의 절대 경로다
  final String path;
  final ChatMessageFileChange change;

  String get name => path.split(RegExp(r'[/\\]')).last;

  /// 모양이 맞지 않는 항목은 메시지 전체를 버리지 않고 그 항목만 건너뛴다
  static ChatMessageFile? tryParse(Object? entry) {
    if (entry is! Map) return null;
    final path = entry['path'];
    final change = ChatMessageFileChange.values
        .where((change) => change.name == entry['change'])
        .firstOrNull;
    if (path is! String || path.isEmpty || change == null) return null;
    return ChatMessageFile(path: path, change: change);
  }
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

class ChatAttachment {
  const ChatAttachment({
    required this.id,
    required this.kind,
    required this.name,
    this.mimeType,
    this.sizeBytes,
  });
  final String id, kind, name;
  final String? mimeType;
  final int? sizeBytes;
  factory ChatAttachment.fromJson(Map<String, dynamic> value) => ChatAttachment(
    id: value['id'] as String,
    kind: value['kind'] as String,
    name: value['name'] as String,
    mimeType: value['mimeType'] as String?,
    sizeBytes: value['sizeBytes'] as int?,
  );
}
