/// PC를 다시 조회할 수 있는 알림 목적지. 인증 토큰이나 임의 접속 주소는 담지 않는다.
class TaskNotification {
  const TaskNotification({
    required this.eventId,
    required this.connectionId,
    required this.taskId,
    required this.title,
    required this.body,
    required this.status,
    required this.createdAt,
  });

  final String eventId;
  final String connectionId;
  final String taskId;
  final String title;
  final String body;
  final String status;
  final String createdAt;

  String get dedupeKey => '$connectionId/$eventId';

  Map<String, dynamic> toJson() => {
    'type': 'task-status-changed',
    'eventId': eventId,
    'connectionId': connectionId,
    'taskId': taskId,
    'title': title,
    'body': body,
    'status': status,
    'createdAt': createdAt,
  };

  static TaskNotification? fromJson(Map<String, dynamic> json) {
    if (json['type'] != 'task-status-changed') return null;
    const fields = [
      'eventId',
      'connectionId',
      'taskId',
      'title',
      'body',
      'status',
      'createdAt',
    ];
    if (fields.any(
      (key) => json[key] is! String || (json[key] as String).isEmpty,
    )) {
      return null;
    }
    return TaskNotification(
      eventId: json['eventId'] as String,
      connectionId: json['connectionId'] as String,
      taskId: json['taskId'] as String,
      title: json['title'] as String,
      body: json['body'] as String,
      status: json['status'] as String,
      createdAt: json['createdAt'] as String,
    );
  }
}
