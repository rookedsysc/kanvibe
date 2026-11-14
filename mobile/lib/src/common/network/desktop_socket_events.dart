import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

/// 어느 PC의 상태 소켓에서 온 이벤트인지와 그 내용
typedef DesktopSocketEvent = ({
  String connectionId,
  Map<String, dynamic> event,
});

/// 상태 소켓으로 받은 이벤트를 화면 쪽 기능에 나눠 준다.
///
/// 소켓은 알림 컨트롤러가 열고 닫는다. 다른 기능이 소켓을 직접 건드리면 연결이 둘로 갈리므로,
/// 받은 이벤트만 여기로 흘려 필요한 쪽이 듣게 한다.
final desktopSocketEventsProvider = Provider<DesktopSocketEvents>((ref) {
  final events = DesktopSocketEvents();
  ref.onDispose(events.close);
  return events;
});

class DesktopSocketEvents {
  final _events = StreamController<DesktopSocketEvent>.broadcast();

  Stream<DesktopSocketEvent> get stream => _events.stream;

  void add(String connectionId, Map<String, dynamic> event) {
    if (_events.isClosed) return;
    _events.add((connectionId: connectionId, event: event));
  }

  Future<void> close() => _events.close();
}
