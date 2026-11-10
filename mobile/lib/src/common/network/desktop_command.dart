/// 상태 WebSocket(`/api/mobile/events`)으로 데스크탑에 보내는 명령 한 건.
///
/// 이름은 데스크탑 `electron/mobileCommands.js`의 핸들러 표와 같아야 하고,
/// 데스크탑이 `connected` 이벤트로 알린 기능 목록에 있을 때만 보낸다.
class DesktopCommand {
  const DesktopCommand._(this.name, this.fields, this.timeout);

  /// 태스크와 그 worktree·브랜치·세션을 지운다. 연결된 데스크탑이 같은 태스크를 가진 다른 데스크탑에도 전파한다.
  ///
  /// 커밋하지 않은 변경이 있으면 `dirty-worktree`로 멈추고, [isForced]로 다시 보내면 그 변경까지 지운다.
  /// 응답의 `failedPeerCount`는 꺼져 있어 전파받지 못한 다른 데스크탑 수다.
  factory DesktopCommand.deleteTask(String taskId, {required bool isForced}) =>
      DesktopCommand._('delete-task', {
        'taskId': taskId,
        'isForced': isForced,
      }, _deleteTaskTimeout);

  /// 탭을 닫기 전에 확인이 필요한지 묻는다. 응답의 `processNames`가 비어 있으면 유휴 셸이라 바로 닫아도 된다
  factory DesktopCommand.getTabCloseInfo(String taskId, String paneId) =>
      DesktopCommand._('get-tab-close-info', {
        'taskId': taskId,
        'paneId': paneId,
      }, _lookupTimeout);

  /// pane 하나(탭 하나)를 닫는다. 응답의 `surfaces`는 닫은 뒤의 탭 목록이다
  factory DesktopCommand.closeTab(String taskId, String paneId) =>
      DesktopCommand._('close-tab', {
        'taskId': taskId,
        'paneId': paneId,
      }, _closeTabTimeout);

  final String name;
  final Map<String, Object?> fields;

  /// 응답을 기다릴 시간. 데스크탑이 하는 일의 길이에 맞춘다
  final Duration timeout;

  /// 데스크탑이 worktree·브랜치·세션을 지우고 다른 데스크탑에까지 전파한 뒤에야 답하므로 조회 잣대로 재지 않는다
  static const _deleteTaskTimeout = Duration(seconds: 90);
  static const _closeTabTimeout = Duration(seconds: 30);
  static const _lookupTimeout = Duration(seconds: 8);

  Map<String, Object?> toMessage(String requestId) => {
    'type': 'command',
    'requestId': requestId,
    'command': name,
    ...fields,
  };
}

/// 열린 이벤트 스트림이 없거나 응답을 기다리던 중 끊겼다. 명령은 보내지 않았거나 결과를 알 수 없다
class DesktopDisconnectedException implements Exception {
  const DesktopDisconnectedException();
}

/// 데스크탑 앱이 이 명령을 모른다. 앱을 올린 쪽보다 데스크탑이 오래된 경우다
class DesktopUpdateRequiredException implements Exception {
  const DesktopUpdateRequiredException();
}

/// 데스크탑이 명령을 거절하거나 실패했다. [code]로 분기하고 [message]는 그대로 보여 줄 수 있다
class DesktopCommandFailure implements Exception {
  const DesktopCommandFailure(this.code, this.message);

  final String code;
  final String message;
}

/// 명령 실패를 화면에 그대로 보여 줄 때 쓰는 안내. 삭제와 탭 닫기가 같은 문장을 쓴다
const desktopUpdateRequiredMessage = '데스크탑 앱을 업데이트해 주세요';
const desktopDisconnectedMessage = '데스크탑과 연결이 끊겼습니다. 연결을 확인해 주세요.';
