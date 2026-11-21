class OperationalWarning {
  const OperationalWarning({
    required this.eventId,
    required this.connectionId,
    required this.kind,
    required this.title,
    required this.body,
    required this.createdAt,
    this.taskId,
    this.shouldNotify = true,
  });
  final String eventId;
  final String connectionId;
  final String kind;
  final String title;
  final String body;
  final String createdAt;
  final String? taskId;
  final bool shouldNotify;
  String get dedupeKey => '$connectionId/$eventId';

  static OperationalWarning? fromJson(Map<String, dynamic> json) {
    if (json['type'] != 'operational-warning' ||
        !const {
          'hook-status-target-missing',
          'task-hook-install-failed',
        }.contains(json['kind'])) {
      return null;
    }
    for (final field in const [
      'eventId',
      'connectionId',
      'title',
      'body',
      'createdAt',
    ]) {
      if (json[field] is! String || (json[field] as String).isEmpty) {
        return null;
      }
    }
    return OperationalWarning(
      eventId: json['eventId'] as String,
      connectionId: json['connectionId'] as String,
      kind: json['kind'] as String,
      title: json['title'] as String,
      body: json['body'] as String,
      createdAt: json['createdAt'] as String,
      taskId: json['taskId'] as String?,
      shouldNotify: json['shouldNotify'] != false,
    );
  }

  Map<String, dynamic> toJson() => {
    'type': 'operational-warning',
    'eventId': eventId,
    'connectionId': connectionId,
    'kind': kind,
    'title': title,
    'body': body,
    'createdAt': createdAt,
    'taskId': taskId,
    'shouldNotify': shouldNotify,
  };
}
