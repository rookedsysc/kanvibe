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

  SavedConnections add(DesktopConnection connection) => SavedConnections(
    connections: [...connections, connection],
    activeId: connection.id,
  );

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
