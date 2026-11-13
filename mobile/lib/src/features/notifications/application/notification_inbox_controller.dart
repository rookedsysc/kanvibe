import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../domain/inbox_notification.dart';
import 'mobile_notification_controller.dart';

final notificationInboxProvider =
    AsyncNotifierProvider<NotificationInboxController, NotificationInboxState>(
      NotificationInboxController.new,
      retry: (_, _) => null,
    );

class NotificationInboxState {
  const NotificationInboxState(this.notifications, {this.isUpdating = false});

  final List<InboxNotification> notifications;
  final bool isUpdating;

  int get unreadCount => notifications.where((entry) => !entry.isRead).length;

  int unreadCountForTask(String taskId) => notifications
      .where((entry) => !entry.isRead && entry.taskId == taskId)
      .length;
}

class NotificationInboxController
    extends AsyncNotifier<NotificationInboxState> {
  int _generation = 0;
  DesktopConnection? _connection;
  bool _refreshPending = false;

  @override
  Future<NotificationInboxState> build() async {
    _generation += 1;
    _refreshPending = false;
    final connections = ref.watch(connectionControllerProvider.future);
    final subscription = ref
        .watch(mobileNotificationControllerProvider)
        .inboxUpdates
        .listen((connectionId) {
          if (!ref.mounted || connectionId != _connection?.id) return;
          _refreshInbox();
        });
    ref.onDispose(() => unawaited(subscription.cancel()));
    final connection = (await connections).activeConnection;
    _connection = connection;
    if (connection == null) return const NotificationInboxState([]);
    try {
      final notifications = await ref
          .read(desktopClientProvider)
          .fetchNotifications(connection);
      return NotificationInboxState(notifications);
    } on DesktopRequestException catch (error) {
      if (error.isUnauthorized) {
        await ref
            .read(connectionControllerProvider.notifier)
            .forgetLocally(connection.id, connection.token);
      }
      rethrow;
    }
  }

  Future<bool> markRead(String id) async {
    final notifications = state.value?.notifications;
    if (notifications == null) return false;
    final matches = notifications.where((entry) => entry.id == id);
    if (matches.isEmpty) return false;
    if (matches.first.isRead) return !state.isLoading;
    return _update((connection, previous) async {
      final updated = await ref
          .read(desktopClientProvider)
          .markNotificationRead(connection, id);
      return previous.map((entry) => entry.id == id ? updated : entry).toList();
    });
  }

  Future<bool> markAllRead() => _update((connection, previous) async {
    await ref.read(desktopClientProvider).markAllNotificationsRead(connection);
    return previous.map((entry) => entry.asRead()).toList();
  });

  /// 태스크 상세를 보고 있을 때 데스크탑과 같은 경로로 그 태스크의 알림을 읽음 처리한다.
  Future<void> markTaskRead(String taskId) async {
    final connections = await ref.read(connectionControllerProvider.future);
    final connection = connections.activeConnection;
    if (connection == null) return;
    await ref
        .read(desktopClientProvider)
        .markTaskNotificationsRead(connection, taskId);
    if (ref.mounted) _refreshInbox();
  }

  void _refreshInbox() {
    // 재조회에는 이전 isUpdating도 남으므로 로딩 중 신호까지 보류하면 갱신을 잃는다.
    if (!state.isLoading && state.value?.isUpdating == true) {
      _refreshPending = true;
    } else {
      ref.invalidateSelf();
    }
  }

  bool _isCurrent(int generation, DesktopConnection connection) {
    if (!ref.mounted || generation != _generation) return false;
    final current = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    return current?.id == connection.id && current?.token == connection.token;
  }

  Future<bool> _update(
    Future<List<InboxNotification>> Function(
      DesktopConnection,
      List<InboxNotification>,
    )
    update,
  ) async {
    final previous = state.value;
    final connection = _connection;
    if (state.isLoading ||
        previous == null ||
        previous.isUpdating ||
        connection == null) {
      return false;
    }
    final generation = _generation;
    state = AsyncData(
      NotificationInboxState(previous.notifications, isUpdating: true),
    );
    try {
      await ref.read(connectionControllerProvider.future);
      if (!_isCurrent(generation, connection)) return false;
      final notifications = await update(connection, previous.notifications);
      if (!_isCurrent(generation, connection)) return false;
      state = AsyncData(NotificationInboxState(notifications));
      if (_refreshPending) ref.invalidateSelf();
      return true;
    } catch (error) {
      if (!_isCurrent(generation, connection)) return false;
      state = AsyncData(previous);
      if (error is DesktopRequestException && error.isUnauthorized) {
        await ref
            .read(connectionControllerProvider.notifier)
            .forgetLocally(connection.id, connection.token);
      } else if (_refreshPending) {
        ref.invalidateSelf();
      }
      rethrow;
    }
  }
}
