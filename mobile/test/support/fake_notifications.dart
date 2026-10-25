import 'dart:async';

import 'package:kanvibe_mobile/src/features/notifications/data/mobile_notifications.dart';

/// OS와 Firebase 대신 수신·클릭을 제어하는 외부 경계 대역이다.
class FakeMobileNotifications implements MobileNotifications {
  final foreground = StreamController<Map<String, dynamic>>.broadcast(
    sync: true,
  );
  final opened = StreamController<Map<String, dynamic>>.broadcast(sync: true);
  final tokens = StreamController<String>.broadcast(sync: true);
  final shown = <Map<String, dynamic>>[];
  Map<String, dynamic>? initialNotification;
  String? pushToken;

  @override
  Stream<Map<String, dynamic>> get foregroundMessages => foreground.stream;

  @override
  Stream<Map<String, dynamic>> get openedNotifications => opened.stream;

  @override
  Stream<String> get tokenChanges => tokens.stream;

  @override
  String get platform => 'android';

  @override
  bool get isPushConfigured => pushToken != null;

  @override
  Future<String?> getPushToken() async => pushToken;

  @override
  Map<String, dynamic>? takeInitialNotification() {
    final notification = initialNotification;
    initialNotification = null;
    return notification;
  }

  @override
  Future<void> show(Map<String, dynamic> notification) async {
    shown.add(notification);
  }

  @override
  Future<void> dispose() async {
    await foreground.close();
    await opened.close();
    await tokens.close();
  }
}

Map<String, dynamic> taskNotification({
  String connectionId = '127.0.0.1:9736',
  String taskId = 'a',
  String eventId = 'event-a',
}) => {
  'type': 'task-status-changed',
  'eventId': eventId,
  'connectionId': connectionId,
  'taskId': taskId,
  'title': '태스크 상태 변경',
  'body': '작업이 검토 대기 상태가 되었습니다.',
  'status': 'review',
  'createdAt': '2026-10-23T01:00:00.000Z',
};
