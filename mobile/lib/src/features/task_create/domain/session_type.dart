/// 태스크가 쓸 터미널 세션 종류.
///
/// 값도 화면에 보이는 이름도 데스크탑 `SessionType`을 그대로 따른다.
/// 데스크탑 생성 화면 역시 이 값을 번역하지 않고 그대로 보여 주므로, 옮겨 쓰면 두 화면의 선택지가 갈린다.
enum SessionType {
  tmux,
  zellij,
  herdr,
  terminal;

  String get wireValue => name;

  /// 데스크탑이 보낸 값에 대응하는 세션 종류. 값을 보내지 않던 이전 데스크탑의 기본값이 tmux라 모르는 값은 tmux로 본다
  static SessionType fromWire(Object? value) => SessionType.values.firstWhere(
    (sessionType) => sessionType.wireValue == value,
    orElse: () => SessionType.tmux,
  );
}
