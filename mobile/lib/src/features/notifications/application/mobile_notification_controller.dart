import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../../connection/domain/saved_connections.dart';
import '../data/mobile_notifications.dart';
import '../domain/task_notification.dart';

/// 네이티브 초기화는 진입점에서 수행하고 테스트는 이 경계를 대체한다.
final mobileNotificationsProvider = Provider<MobileNotifications>(
  (ref) => MobileNotifications.disabled(),
);

final mobileNotificationControllerProvider =
    Provider<MobileNotificationController>((ref) {
      final controller = MobileNotificationController(
        client: ref.read(desktopClientProvider),
        notifications: ref.read(mobileNotificationsProvider),
        onBoardChanged: (connectionId) {
          if (ref.read(connectionControllerProvider).value?.activeId ==
              connectionId) {
            ref.invalidate(boardProvider);
          }
        },
        onDeviceUnpaired: (connection) => ref
            .read(connectionControllerProvider.notifier)
            .forgetLocally(connection.id, connection.token),
      );
      ref.listen(connectionControllerProvider, (_, next) {
        controller.setConnections(next.value ?? const SavedConnections());
      }, fireImmediately: true);
      ref.onDispose(() => unawaited(controller.dispose()));
      return controller;
    });

/// 현재 PC의 상세 화면도 hook으로 변경된 상태를 바로 반영한다.
final taskNotificationUpdatesProvider =
    StreamProvider.family<TaskNotification, String>((ref, taskId) {
      final connectionId = ref
          .watch(connectionControllerProvider)
          .value
          ?.activeId;
      return ref
          .watch(mobileNotificationControllerProvider)
          .taskUpdates
          .where(
            (event) =>
                event.connectionId == connectionId && event.taskId == taskId,
          );
    });

/// 저장된 PC 전체의 전경 수신과 푸시 토큰을 함께 관리한다.
/// 배경에서는 OS 푸시만 사용하고 복귀하면 소켓과 최신 보드를 다시 연결한다.
class MobileNotificationController {
  MobileNotificationController({
    required this.client,
    required this.notifications,
    required this.onBoardChanged,
    required this.onDeviceUnpaired,
  }) {
    _foregroundSubscription = notifications.foregroundMessages.listen(
      _receivePush,
    );
    _openedSubscription = notifications.openedNotifications.listen((json) {
      final notification = TaskNotification.fromJson(json);
      if (!_isDisposed && notification != null) _openRequests.add(notification);
    });
    _tokenSubscription = notifications.tokenChanges.listen((token) {
      _tokenGeneration += 1;
      _updatePushToken(token);
    });
    unawaited(_refreshPushToken());
  }

  final DesktopClient client;
  final MobileNotifications notifications;
  final void Function(String connectionId) onBoardChanged;
  final Future<void> Function(DesktopConnection connection) onDeviceUnpaired;
  final _openRequests = StreamController<TaskNotification>.broadcast();
  final _taskUpdates = StreamController<TaskNotification>.broadcast();
  final _connections = <String, DesktopConnection>{};
  final _sessions = <String, _EventSession>{};
  final _registrationQueues = <String, Future<void>>{};
  final _seenEvents = <String>{};
  late final StreamSubscription<Map<String, dynamic>> _foregroundSubscription;
  late final StreamSubscription<Map<String, dynamic>> _openedSubscription;
  late final StreamSubscription<String> _tokenSubscription;
  String? _pushToken;
  int _tokenGeneration = 0;
  bool _isActive = true;
  bool _isDisposed = false;
  static const _retainedEventCount = 256;

  Stream<TaskNotification> get openRequests => _openRequests.stream;
  Stream<TaskNotification> get taskUpdates => _taskUpdates.stream;

  TaskNotification? takeInitialNotification() {
    final json = notifications.takeInitialNotification();
    return json == null ? null : TaskNotification.fromJson(json);
  }

  void setConnections(SavedConnections registry) {
    if (_isDisposed) return;
    final previous = Map<String, DesktopConnection>.of(_connections);
    _connections
      ..clear()
      ..addEntries(
        registry.connections.map(
          (connection) => MapEntry(connection.id, connection),
        ),
      );
    for (final entry in previous.entries) {
      if (_connections[entry.key]?.token != entry.value.token) {
        _stopSession(entry.key);
      }
    }
    for (final connection in registry.connections) {
      if (_isActive && !_sessions.containsKey(connection.id)) {
        _startSession(connection);
      }
      if (previous[connection.id]?.token != connection.token) {
        unawaited(_registerNotifications(connection));
      }
    }
  }

  void setActive(bool isActive) {
    if (_isDisposed || isActive == _isActive) return;
    _isActive = isActive;
    if (!isActive) {
      for (final id in _sessions.keys.toList()) {
        _stopSession(id);
      }
      return;
    }
    for (final connection in _connections.values) {
      _startSession(connection);
      onBoardChanged(connection.id);
    }
    unawaited(_refreshPushToken());
  }

  bool _isCurrent(DesktopConnection connection) =>
      !_isDisposed && _connections[connection.id]?.token == connection.token;

  void _startSession(DesktopConnection connection) {
    if (!_isActive || !_isCurrent(connection)) return;
    final session = _EventSession(connection);
    _sessions[connection.id] = session;
    _connectSession(session);
  }

  void _connectSession(_EventSession session) {
    if (!_isActive || !_isCurrent(session.connection) || session.isClosed) {
      return;
    }
    try {
      final stream = client.openEventStream(session.connection);
      session.stream = stream;
      session.subscription = stream.events.listen(
        (json) => _receiveEvent(session, json),
        onError: (Object error) {
          if (error is DesktopRequestException && error.isUnauthorized) {
            unawaited(_rejectConnection(session));
          } else {
            _scheduleReconnect(session);
          }
        },
        onDone: () {
          if (stream.readCloseCode() == deviceUnpairedCloseCode) {
            unawaited(_rejectConnection(session));
          } else {
            _scheduleReconnect(session);
          }
        },
      );
    } catch (_) {
      _scheduleReconnect(session);
    }
  }

  void _receiveEvent(_EventSession session, Map<String, dynamic> json) {
    if (session.isClosed || !_isActive || !_isCurrent(session.connection)) {
      return;
    }
    if (json['type'] == 'connected' || json['type'] == 'board-updated') {
      session.retrySeconds = 1;
      onBoardChanged(session.connection.id);
      if (json['type'] == 'connected') {
        unawaited(_registerNotifications(session.connection));
      }
      return;
    }
    final notification = TaskNotification.fromJson({
      ...json,
      'connectionId': session.connection.id,
    });
    if (notification != null) _receiveTaskNotification(notification);
  }

  void _receivePush(Map<String, dynamic> json) {
    final notification = TaskNotification.fromJson(json);
    if (notification != null) _receiveTaskNotification(notification);
  }

  void _receiveTaskNotification(TaskNotification notification) {
    if (_isDisposed ||
        !_isActive ||
        !_connections.containsKey(notification.connectionId)) {
      return;
    }
    if (!_seenEvents.add(notification.dedupeKey)) return;
    if (_seenEvents.length > _retainedEventCount) {
      _seenEvents.remove(_seenEvents.first);
    }
    onBoardChanged(notification.connectionId);
    _taskUpdates.add(notification);
    unawaited(_showNotification(notification));
  }

  Future<void> _showNotification(TaskNotification notification) async {
    try {
      await notifications.show(notification.toJson());
    } catch (_) {
      debugPrint('KanVibe: 상태 알림을 표시하지 못했습니다.');
    }
  }

  void _scheduleReconnect(_EventSession session) {
    if (session.isClosed ||
        !_isActive ||
        !_isCurrent(session.connection) ||
        session.retry != null) {
      return;
    }
    unawaited(session.subscription?.cancel());
    unawaited(session.stream?.close());
    session.retry = Timer(Duration(seconds: session.retrySeconds), () {
      session.retry = null;
      _connectSession(session);
    });
    session.retrySeconds = (session.retrySeconds * 2).clamp(1, 30);
  }

  Future<void> _rejectConnection(_EventSession session) async {
    if (!_isCurrent(session.connection) || session.isClosed) return;
    _stopSession(session.connection.id);
    await _forgetConnection(session.connection);
  }

  void _stopSession(String id) {
    final session = _sessions.remove(id);
    if (session == null) return;
    session.isClosed = true;
    session.retry?.cancel();
    unawaited(session.subscription?.cancel());
    unawaited(session.stream?.close());
  }

  Future<void> _refreshPushToken() async {
    final generation = ++_tokenGeneration;
    try {
      final token = await notifications.getPushToken();
      if (_isDisposed || generation != _tokenGeneration) return;
      _updatePushToken(token);
    } catch (_) {
      debugPrint('KanVibe: 푸시 수신 설정을 확인하지 못했습니다.');
    }
  }

  void _updatePushToken(String? token) {
    if (_isDisposed) return;
    _pushToken = token;
    for (final connection in _connections.values) {
      unawaited(_registerNotifications(connection));
    }
  }

  /// 이전 토큰 등록이 늦게 도착해 갱신한 값을 덮어쓰지 않도록 PC별 쓰기를 직렬화한다.
  Future<void> _registerNotifications(DesktopConnection connection) {
    final previous = _registrationQueues[connection.id] ?? Future<void>.value();
    final next = previous.then((_) => _sendRegistration(connection));
    _registrationQueues[connection.id] = next;
    unawaited(
      next.whenComplete(() {
        if (identical(_registrationQueues[connection.id], next)) {
          _registrationQueues.remove(connection.id);
        }
      }),
    );
    return next;
  }

  Future<void> _sendRegistration(DesktopConnection connection) async {
    final token = _pushToken;
    if (!_isCurrent(connection) || token == null) return;
    try {
      await client.registerNotifications(
        connection,
        pushToken: token,
        platform: notifications.platform,
      );
    } on DesktopRequestException catch (error) {
      if (error.isUnauthorized && _isCurrent(connection)) {
        await _forgetConnection(connection);
      }
    } catch (_) {
      debugPrint('KanVibe: PC에 푸시 수신 정보를 등록하지 못했습니다.');
    }
  }

  Future<void> _forgetConnection(DesktopConnection connection) async {
    try {
      await onDeviceUnpaired(connection);
    } catch (_) {
      debugPrint('KanVibe: 해제된 PC 연결을 정리하지 못했습니다.');
    }
  }

  Future<void> dispose() async {
    if (_isDisposed) return;
    _isDisposed = true;
    for (final id in _sessions.keys.toList()) {
      _stopSession(id);
    }
    await _foregroundSubscription.cancel();
    await _openedSubscription.cancel();
    await _tokenSubscription.cancel();
    await _openRequests.close();
    await _taskUpdates.close();
  }
}

class _EventSession {
  _EventSession(this.connection);

  final DesktopConnection connection;
  DesktopEventStream? stream;
  StreamSubscription<Map<String, dynamic>>? subscription;
  Timer? retry;
  int retrySeconds = 1;
  bool isClosed = false;
}
