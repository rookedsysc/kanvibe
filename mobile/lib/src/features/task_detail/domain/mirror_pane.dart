import '../../task_create/domain/session_type.dart';

/// 비출 pane 하나. 좌표는 태블릿이 window 배치를 재현할 때 쓴다
class MirrorPane {
  const MirrorPane({
    required this.id,
    required this.tabId,
    required this.tabName,
    required this.command,
    required this.left,
    required this.top,
    required this.width,
    required this.height,
  });

  final String id;
  final String tabId;
  final String tabName;
  final String command;
  final int left;
  final int top;
  final int width;
  final int height;

  static MirrorPane? fromJson(Map<String, dynamic> json) {
    final id = json['id'];
    if (id is! String) {
      return null;
    }

    return MirrorPane(
      id: id,
      tabId: json['tabId']?.toString() ?? '',
      tabName: json['tabName']?.toString() ?? '',
      command: json['command']?.toString() ?? '',
      left: _readInt(json['left']),
      top: _readInt(json['top']),
      width: _readInt(json['width']),
      height: _readInt(json['height']),
    );
  }

  static int _readInt(Object? value) => value is num ? value.toInt() : 0;
}

/// tmux window, zellij tab 또는 Herdr tab 하나
class MirrorTab {
  const MirrorTab({required this.id, required this.name, required this.panes});

  final String id;
  final String name;
  final List<MirrorPane> panes;

  static MirrorTab fromJson(Map<String, dynamic> json) => MirrorTab(
    id: json['id']?.toString() ?? '',
    name: json['name']?.toString() ?? '',
    panes: (json['panes'] as List<dynamic>? ?? [])
        .whereType<Map<String, dynamic>>()
        .map(MirrorPane.fromJson)
        .whereType<MirrorPane>()
        .toList(),
  );
}

/// 태스크가 가진 탭과 pane 전부
class TaskSurfaces {
  const TaskSurfaces({
    required this.tabs,
    this.canResize = false,
    this.canRefreshHistory = false,
    this.canPrepareDirectConnection = true,
    this.sessionType = SessionType.terminal,
    this.canRespondToDeviceQueries = false,
    this.geometryTabId,
  });

  final List<MirrorTab> tabs;
  final bool canResize;

  /// 데스크탑이 화면 프레임만 보내 지난 출력이 폰에 스스로 쌓이지 않는 세션이다
  final bool canRefreshHistory;

  /// 터미널을 열기 전에 B로 가는 직접 연결 권한을 물어볼 세션이다.
  /// 이 값을 보내지 않는 옛 데스크탑에는 전처럼 매번 물어본다
  final bool canPrepareDirectConnection;
  final SessionType sessionType;
  final bool canRespondToDeviceQueries;

  /// A minimal response certifies geometry for this tab only; legacy/full responses omit it.
  final String? geometryTabId;

  /// 폰이 쓰는 목록. 모든 window에 걸친 pane을 순서대로 펼친다
  List<MirrorPane> get allPanes => [for (final tab in tabs) ...tab.panes];

  static TaskSurfaces fromJson(Map<String, dynamic> json) => TaskSurfaces(
    sessionType:
        SessionType.values
            .where((type) => type.name == json['sessionType'])
            .firstOrNull ??
        SessionType.terminal,
    geometryTabId: json['geometryTabId'] is String
        ? json['geometryTabId'] as String
        : null,
    canResize: json['canResize'] == true,
    canRefreshHistory: json['canRefreshHistory'] == true,
    canPrepareDirectConnection: json['canPrepareDirectConnection'] != false,
    canRespondToDeviceQueries: json['sessionType'] == 'terminal',
    tabs: (json['tabs'] as List<dynamic>? ?? [])
        .whereType<Map<String, dynamic>>()
        .map(MirrorTab.fromJson)
        .toList(),
  );
}

/// 세션을 비출 수 없는 이유. 화면은 빈 터미널 대신 이 사유를 보여 준다
enum SurfaceUnavailableReason {
  noSession,
  sessionNotRunning,
  zellijTooOld,
  herdrMissing,
  remoteUnreachable,
  unknown;

  static SurfaceUnavailableReason fromWire(String? value) => switch (value) {
    'no-session' => SurfaceUnavailableReason.noSession,
    'session-not-running' => SurfaceUnavailableReason.sessionNotRunning,
    'zellij-too-old' => SurfaceUnavailableReason.zellijTooOld,
    'herdr-missing' => SurfaceUnavailableReason.herdrMissing,
    'remote-unreachable' => SurfaceUnavailableReason.remoteUnreachable,
    _ => SurfaceUnavailableReason.unknown,
  };
}

/// 세션을 비출 수 없을 때 던진다
class SurfaceUnavailableException implements Exception {
  const SurfaceUnavailableException(this.reason);

  final SurfaceUnavailableReason reason;
}
