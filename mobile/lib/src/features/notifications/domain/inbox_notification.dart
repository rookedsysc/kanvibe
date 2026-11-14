class InboxNotification {
  const InboxNotification({
    required this.id,
    required this.title,
    required this.body,
    required this.taskId,
    required this.isRead,
    required this.createdAt,
    this.actionType,
  });

  final String id;
  final String title;
  final String body;
  final String? taskId;
  final bool isRead;
  final DateTime createdAt;
  final String? actionType;
  bool get hasSyncReview => actionType == 'background-sync-review';

  InboxNotification asRead() => InboxNotification(
    id: id,
    title: title,
    body: body,
    taskId: taskId,
    isRead: true,
    createdAt: createdAt,
    actionType: actionType,
  );

  static InboxNotification? fromJson(Map<String, dynamic> json) {
    final id = json['id'];
    final title = json['title'];
    final body = json['body'];
    final taskId = json['taskId'];
    final isRead = json['isRead'];
    final createdAt = json['createdAt'];
    if (id is! String ||
        id.isEmpty ||
        title is! String ||
        body is! String ||
        (taskId != null && taskId is! String) ||
        isRead is! bool ||
        createdAt is! String) {
      return null;
    }
    final timestamp = DateTime.tryParse(createdAt);
    if (timestamp == null) return null;
    return InboxNotification(
      id: id,
      title: title,
      body: body,
      taskId: taskId is String && taskId.isNotEmpty ? taskId : null,
      isRead: isRead,
      createdAt: timestamp,
      actionType: json['actionType'] is String
          ? json['actionType'] as String
          : null,
    );
  }
}
