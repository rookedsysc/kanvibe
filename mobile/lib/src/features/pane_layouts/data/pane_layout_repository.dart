import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../domain/pane_layout.dart';

class PaneLayoutRepository {
  const PaneLayoutRepository(this.client);
  final DesktopClient client;
  PaneLayout _parse(Map<String, dynamic> json) {
    final panes =
        (json['panes'] as List<dynamic>? ?? [])
            .whereType<Map<String, dynamic>>()
            .toList()
          ..sort(
            (left, right) =>
                (left['position'] as int).compareTo(right['position'] as int),
          );
    return PaneLayout(
      id: json['id'] as String,
      kind: PaneLayoutKind.values.firstWhere(
        (kind) => kind.wireValue == json['layoutType'],
      ),
      commands: panes.map((pane) => pane['command'] as String).toList(),
      isGlobal: json['isGlobal'] == true,
      projectId: json['projectId'] as String?,
    );
  }

  Future<List<PaneLayout>> fetch(DesktopConnection connection) async {
    final response = await client.requestJson(connection, 'GET', [
      'pane-layouts',
    ]);
    return (response['layouts'] as List<dynamic>? ?? [])
        .whereType<Map<String, dynamic>>()
        .map(_parse)
        .toList();
  }

  Future<void> save(
    DesktopConnection connection,
    PaneLayoutKind kind,
    List<String> commands,
    String? projectId,
  ) async {
    await client.requestJson(
      connection,
      'PUT',
      ['pane-layouts'],
      body: {
        'layoutType': kind.wireValue,
        'isGlobal': projectId == null,
        'projectId': ?projectId,
        'panes': [
          for (var index = 0; index < kind.paneCount; index++)
            {'position': index, 'command': commands[index]},
        ],
      },
    );
  }

  Future<void> delete(DesktopConnection connection, String id) async {
    await client.requestJson(connection, 'DELETE', ['pane-layouts', id]);
  }
}
