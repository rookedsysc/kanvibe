import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_command.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/network/desktop_socket_events.dart';
import '../../../common/providers.dart';
import '../domain/pane_chat.dart';

/// 데스크탑이 변화를 밀어 주는 pane 구독을 한곳에서 관리한다
final paneChatSubscriptionsProvider = Provider<PaneChatSubscriptions>((ref) {
  final subscriptions = PaneChatSubscriptions(ref);
  ref.onDispose(subscriptions.dispose);
  return subscriptions;
});

/// pane 하나를 push로 받으려는 쪽이 등록하는 내용
class PaneChatSubscriber {
  PaneChatSubscriber({
    required this.target,
    required this.includesChat,
    required this.onEvent,
    required this.onLiveChanged,
    this.wantsScreen = false,
    this.readCursor,
  });

  final PaneChatTarget target;

  /// false면 에이전트가 돌고 있는지만 받는다
  final bool includesChat;

  /// CLI 화면이 실제로 보이는 동안에만 켠다
  final bool wantsScreen;

  /// 대화를 어디까지 가졌는지. 데스크탑이 이 위치로 다시 받을지를 판단한다
  final PaneChatCursor? Function()? readCursor;

  /// 이 pane의 `pane-chat-*` 이벤트
  final void Function(Map<String, dynamic> event) onEvent;

  /// push가 살아 있는지가 바뀌면 불린다. 살아 있지 않은 동안에는 등록한 쪽이 주기적으로 조회해야 한다.
  /// [isBehind]는 push는 붙었지만 가진 내용이 뒤처져 한 번 다시 받아야 한다는 뜻이다
  final void Function({required bool isLive, required bool isBehind})
  onLiveChanged;
}

/// 화면에 보이는 pane 전체를 데스크탑에 통째로 알리고, 받은 이벤트를 pane별로 나눠 준다.
///
/// 데스크탑은 놓친 이벤트를 다시 보내지 않는다. 그래서 소켓이 다시 붙을 때마다 가진 위치를 실어 구독을 다시 보내고,
/// 뒤처졌다는 답을 받은 쪽이 스스로 다시 조회한다.
class PaneChatSubscriptions {
  PaneChatSubscriptions(this._ref) {
    _events = _ref.read(desktopSocketEventsProvider).stream.listen(_receive);
  }

  final Ref _ref;
  late final StreamSubscription<DesktopSocketEvent> _events;
  final _subscribers = <PaneChatTarget, PaneChatSubscriber>{};
  final _livePanes = <PaneChatTarget>{};
  bool _isSyncScheduled = false;
  int _syncGeneration = 0;
  bool _isDisposed = false;

  /// 같은 pane을 다시 등록하면 조건만 바뀐 것으로 보고 바꿔 끼운다
  void subscribe(PaneChatSubscriber subscriber) {
    _subscribers[subscriber.target] = subscriber;
    _scheduleSync();
  }

  void unsubscribe(PaneChatSubscriber subscriber) {
    if (!identical(_subscribers[subscriber.target], subscriber)) return;
    _subscribers.remove(subscriber.target);
    _livePanes.remove(subscriber.target);
    _scheduleSync();
  }

  void dispose() {
    _isDisposed = true;
    unawaited(_events.cancel());
  }

  DesktopConnection? get _activeConnection =>
      _ref.read(connectionControllerProvider).value?.activeConnection;

  void _receive(DesktopSocketEvent received) {
    if (_isDisposed || received.connectionId != _activeConnection?.id) return;
    final event = received.event;
    switch (event['type']) {
      case 'connected':
        _scheduleSync();
      case 'disconnected':
        _markAllOffline();
      default:
        final target = (
          taskId: event['taskId']?.toString() ?? '',
          paneId: event['paneId']?.toString() ?? '',
        );
        if (_livePanes.contains(target)) _subscribers[target]?.onEvent(event);
    }
  }

  void _markAllOffline() {
    _syncGeneration++;
    final offline = _livePanes.toList();
    _livePanes.clear();
    for (final target in offline) {
      _subscribers[target]?.onLiveChanged(isLive: false, isBehind: false);
    }
  }

  // 한 프레임에 여러 pane이 등록·해제돼도 구독 명령은 한 번만 보낸다
  void _scheduleSync() {
    if (_isSyncScheduled || _isDisposed) return;
    _isSyncScheduled = true;
    scheduleMicrotask(() {
      _isSyncScheduled = false;
      if (!_isDisposed) unawaited(_sync());
    });
  }

  Future<void> _sync() async {
    final generation = ++_syncGeneration;
    final List<PaneChatSubscriber> requested;
    final Map<String, dynamic> result;
    try {
      // 앱을 켠 직후에는 저장된 연결을 아직 읽는 중일 수 있어 기다렸다가 보낸다
      final connections = await _ref.read(connectionControllerProvider.future);
      final connection = connections.activeConnection;
      if (_isDisposed || generation != _syncGeneration || connection == null) {
        return;
      }
      requested = _subscribers.values.toList();
      result = await _ref
          .read(desktopClientProvider)
          .runCommand(
            connection,
            DesktopCommand.setPaneChatSubscription([
              for (final subscriber in requested)
                {
                  'taskId': subscriber.target.taskId,
                  'paneId': subscriber.target.paneId,
                  'detail': subscriber.includesChat ? 'chat' : 'agent',
                  'wantsScreen': subscriber.wantsScreen,
                  if (subscriber.readCursor?.call() case final cursor?)
                    'cursor': {'epoch': cursor.epoch, 'seq': cursor.seq},
                },
            ]),
          );
    } catch (_) {
      // 구독을 모르는 데스크탑이거나 소켓이 없는 동안에는 등록한 쪽이 주기 조회로 돌아간다
      if (!_isDisposed && generation == _syncGeneration) _markAllOffline();
      return;
    }
    if (_isDisposed || generation != _syncGeneration) return;
    final behindPanes = {
      for (final pane in result['panes'] as List<dynamic>? ?? const [])
        if (pane is Map<String, dynamic> && pane['freshness'] != 'current')
          (
            taskId: pane['taskId']?.toString() ?? '',
            paneId: pane['paneId']?.toString() ?? '',
          ),
    };
    for (final subscriber in requested) {
      if (!identical(_subscribers[subscriber.target], subscriber)) continue;
      final wasLive = !_livePanes.add(subscriber.target);
      final isBehind = behindPanes.contains(subscriber.target);
      if (!wasLive || isBehind) {
        subscriber.onLiveChanged(isLive: true, isBehind: isBehind);
      }
    }
  }
}
