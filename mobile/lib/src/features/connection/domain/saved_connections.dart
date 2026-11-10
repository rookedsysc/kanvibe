import '../../../common/network/desktop_connection.dart';

/// 이 기기에 페어링된 PC와 지금 보고 있는 PC의 식별자.
class SavedConnections {
  const SavedConnections({this.connections = const [], this.activeId});

  final List<DesktopConnection> connections;
  final String? activeId;

  DesktopConnection? get activeConnection {
    for (final connection in connections) {
      if (connection.id == activeId) return connection;
    }
    return null;
  }

  DesktopConnection? find(String id) {
    for (final connection in connections) {
      if (connection.id == id) return connection;
    }
    return null;
  }

  /// 같은 PC에 TLS 도입 전에 페어링해 평문으로 통신하는 연결들
  List<DesktopConnection> legacyConnectionsOf(String host) {
    final normalizedHost = host.trim().toLowerCase();
    return connections
        .where(
          (connection) =>
              !connection.isEncrypted &&
              connection.host.trim().toLowerCase() == normalizedHost,
        )
        .toList();
  }

  /// 암호화 연결을 추가하면서 같은 PC의 평문 연결을 지운다.
  /// 남겨 두면 알림 스트림이 옛 연결로도 열려 기기 토큰이 계속 평문으로 나간다.
  SavedConnections replaceLegacyConnections(DesktopConnection connection) {
    final legacyIds = connection.isEncrypted
        ? legacyConnectionsOf(connection.host)
              .map((legacy) => legacy.id)
              .toSet()
        : const <String>{};
    return SavedConnections(
      connections: [
        ...connections.where((entry) => !legacyIds.contains(entry.id)),
        connection,
      ],
      activeId: connection.id,
    );
  }

  SavedConnections select(String id) {
    if (find(id) == null) throw ArgumentError.value(id, 'id');
    return SavedConnections(connections: connections, activeId: id);
  }

  SavedConnections remove(String id) => SavedConnections(
    connections: connections
        .where((connection) => connection.id != id)
        .toList(),
    activeId: activeId == id ? null : activeId,
  );

  Map<String, dynamic> toJson() => {
    'activeId': activeId,
    'connections': connections
        .map((connection) => connection.toJson())
        .toList(),
  };

  static SavedConnections? fromJson(Object? value) {
    if (value is! Map<String, dynamic>) return null;
    final rawConnections = value['connections'];
    if (rawConnections is! List) return null;

    final connections = <DesktopConnection>[];
    for (final raw in rawConnections) {
      if (raw is! Map<String, dynamic>) return null;
      final connection = DesktopConnection.fromJson(raw);
      if (connection == null ||
          connections.any((entry) => entry.id == connection.id)) {
        return null;
      }
      connections.add(connection);
    }

    final activeId = value['activeId'];
    if (activeId != null && activeId is! String) return null;
    return SavedConnections(
      connections: connections,
      activeId: connections.any((connection) => connection.id == activeId)
          ? activeId as String
          : null,
    );
  }
}
