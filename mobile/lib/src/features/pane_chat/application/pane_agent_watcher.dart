import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/providers.dart';
import '../domain/pane_chat.dart';
import 'pane_chat_controller.dart';
import 'pane_chat_subscriptions.dart';

/// pane에서 돌고 있는 에이전트 CLI의 이름. 없거나 아직 확인하지 못했으면 null이다
final paneAgentProvider = NotifierProvider.autoDispose
    .family<PaneAgentWatcher, String?, PaneChatTarget>(PaneAgentWatcher.new);

/// 터미널을 보는 동안 pane의 에이전트 실행 여부를 따라간다.
///
/// 데스크탑이 변화를 밀어 주면 그것을 받고, 구독을 모르는 데스크탑이거나 소켓이 끊긴 동안에는 주기적으로 확인한다.
///
/// pane 목록의 command 값은 Claude 네이티브 설치나 zellij·Herdr에서 에이전트를 가리키지 못하므로,
/// 데스크탑이 프로세스 트리로 판정해 주는 pane 채팅 조회 결과를 쓴다.
class PaneAgentWatcher extends Notifier<String?> {
  PaneAgentWatcher(this.target);
  final PaneChatTarget target;

  static const _probeInterval = Duration(seconds: 5);
  Timer? _probeTimer;

  /// 데스크탑이 변화를 밀어 주고 있는지. 그동안에는 주기적으로 확인하지 않는다
  bool _isLive = false;

  @override
  String? build() {
    final subscriptions = ref.read(paneChatSubscriptionsProvider);
    final subscriber = PaneChatSubscriber(
      target: target,
      includesChat: false,
      onEvent: (event) {
        final pushed = event['state'];
        if (event['type'] == 'pane-chat-state' &&
            pushed is Map<String, dynamic> &&
            pushed.containsKey('provider')) {
          state = pushed['provider'] as String?;
        }
      },
      onLiveChanged: ({required isLive, required isBehind}) {
        _isLive = isLive;
        _probeTimer?.cancel();
        if (!isLive) unawaited(_probe());
      },
    );
    subscriptions.subscribe(subscriber);
    ref.onDispose(() {
      _probeTimer?.cancel();
      subscriptions.unsubscribe(subscriber);
    });
    unawaited(_probe());
    // 채팅을 보다가 터미널로 돌아온 직후에는 채팅이 마지막으로 확인한 값으로 시작해 버튼이 깜빡이지 않게 한다
    final chat = paneChatProvider(target);
    return ref.exists(chat) ? ref.read(chat).value?.snapshot.provider : null;
  }

  Future<void> _probe() async {
    try {
      final connections = await ref.read(connectionControllerProvider.future);
      final connection = connections.activeConnection;
      if (!ref.mounted) return;
      if (connection != null) {
        final snapshot = await ref
            .read(desktopClientProvider)
            .fetchPaneChat(connection, target);
        if (!ref.mounted) return;
        state = snapshot.provider;
      }
    } catch (_) {
      // 일시적인 조회 실패로 버튼이 사라지지 않도록 마지막으로 확인한 값을 유지한다
    }
    if (ref.mounted && !_isLive) {
      _probeTimer?.cancel();
      _probeTimer = Timer(_probeInterval, () => unawaited(_probe()));
    }
  }
}
