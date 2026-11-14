import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/features/notifications/domain/inbox_notification.dart';

import 'fake_desktop.dart';

InboxNotification inboxNotice({
  String id = 'n1',
  String? taskId = 'task-1',
  bool isRead = false,
}) => InboxNotification(
  id: id,
  title: '알림 $id',
  body: '데스크톱에서 보낸 알림 내용',
  taskId: taskId,
  isRead: isRead,
  createdAt: DateTime.utc(2026, 11, 1, 8),
);

class InboxDesktopClient extends FakeDesktopClient {
  InboxDesktopClient({super.tasksById, super.board, super.surfaces});
  final notificationsByConnection = <String, List<InboxNotification>>{};
  final inboxFetches = <String>[];
  final readCalls = <({String connectionId, String id})>[];
  final readAllCalls = <String>[];
  final readTaskCalls = <({String connectionId, String taskId})>[];
  Future<List<InboxNotification>> Function(DesktopConnection)? onFetch;
  Future<void> Function()? onRead;
  Future<void> Function()? onReadAll;

  @override
  Future<List<InboxNotification>> fetchNotifications(
    DesktopConnection connection,
  ) async {
    inboxFetches.add(connection.id);
    return onFetch == null
        ? List.of(notificationsByConnection[connection.id] ?? [])
        : await onFetch!(connection);
  }

  @override
  Future<InboxNotification> markNotificationRead(
    DesktopConnection connection,
    String id,
  ) async {
    readCalls.add((connectionId: connection.id, id: id));
    await onRead?.call();
    final current = notificationsByConnection[connection.id]!;
    final updated = current.firstWhere((entry) => entry.id == id).asRead();
    notificationsByConnection[connection.id] = current
        .map((entry) => entry.id == id ? updated : entry)
        .toList();
    return updated;
  }

  @override
  Future<void> markAllNotificationsRead(DesktopConnection connection) async {
    readAllCalls.add(connection.id);
    await onReadAll?.call();
    notificationsByConnection[connection.id] =
        (notificationsByConnection[connection.id] ?? [])
            .map((entry) => entry.asRead())
            .toList();
  }

  @override
  Future<void> markTaskNotificationsRead(
    DesktopConnection connection,
    String taskId,
  ) async {
    readTaskCalls.add((connectionId: connection.id, taskId: taskId));
    notificationsByConnection[connection.id] =
        (notificationsByConnection[connection.id] ?? [])
            .map((entry) => entry.taskId == taskId ? entry.asRead() : entry)
            .toList();
  }
}
