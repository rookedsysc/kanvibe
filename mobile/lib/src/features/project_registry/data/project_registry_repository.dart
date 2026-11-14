import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../../board/domain/board_project.dart';
import 'project_scan_result.dart';

class ProjectRegistryRepository {
  const ProjectRegistryRepository(this.client);
  final DesktopClient client;
  Future<List<String>> fetchHosts(DesktopConnection connection) async =>
      List<String>.unmodifiable(
        (await client.requestJson(connection, 'GET', ['hosts']))['hosts']
            as List,
      );
  Future<List<String>> browse(
    DesktopConnection connection,
    String path,
    String? sshHost,
  ) async {
    if (sshHost == null) {
      return client.fetchProjectDirectories(connection, path);
    }
    final value = await client.requestJson(
      connection,
      'GET',
      ['directories'],
      query: {'path': path, 'sshHost': sshHost},
    );
    return List<String>.unmodifiable(value['directories'] as List);
  }

  Future<ProjectScanResult> scan(
    DesktopConnection connection,
    String path,
    String? sshHost,
  ) async {
    if (sshHost == null) return client.scanProjects(connection, path);
    final value = await client.requestJson(
      connection,
      'POST',
      ['projects', 'scan'],
      body: {'rootPath': path, 'sshHost': sshHost},
      timeout: const Duration(minutes: 5),
    );
    return ProjectScanResult.fromJson(
      Map<String, dynamic>.from(value['result'] as Map),
    );
  }

  Future<List<BoardProject>> fetchProjects(DesktopConnection connection) async {
    final board = await client.fetchBoard(connection);
    return List.unmodifiable(
      (board['projects'] as List? ?? [])
          .map(
            (raw) =>
                BoardProject.fromJson(Map<String, dynamic>.from(raw as Map)),
          )
          .whereType<BoardProject>(),
    );
  }

  Future<void> remove(DesktopConnection connection, String projectId) async {
    await client.requestJson(connection, 'DELETE', ['projects', projectId]);
  }
}
