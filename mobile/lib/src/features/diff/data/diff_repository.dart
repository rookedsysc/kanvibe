import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../domain/git_diff.dart';

class DiffRepository {
  const DiffRepository(this.client);
  final DesktopClient client;

  Future<List<ChangedFile>> fetchFiles(
    DesktopConnection connection,
    String taskId,
  ) async {
    final response = await client.requestJson(connection, 'GET', [
      'tasks',
      taskId,
      'diff',
    ]);
    return (response['files'] as List<dynamic>? ?? [])
        .whereType<Map<String, dynamic>>()
        .map(
          (json) => ChangedFile(
            path: json['path'] as String,
            status: json['status'] as String,
            originalPath: json['originalPath'] as String?,
            additions: (json['additions'] as num?)?.toInt() ?? 0,
            deletions: (json['deletions'] as num?)?.toInt() ?? 0,
          ),
        )
        .toList();
  }

  Future<DiffFileContents> fetchFile(
    DesktopConnection connection,
    String taskId,
    String filePath,
  ) async {
    final response = await client.requestJson(
      connection,
      'GET',
      ['tasks', taskId, 'files'],
      query: {'path': filePath},
    );
    return DiffFileContents(
      original: response['original'] as String,
      content: response['content'] as String,
    );
  }

  Future<void> save(
    DesktopConnection connection,
    String taskId,
    String filePath,
    String content,
  ) async {
    await client.requestJson(
      connection,
      'PUT',
      ['tasks', taskId, 'files'],
      query: {'path': filePath},
      body: {'content': content},
    );
  }

  Future<Map<String, TaskChangeStats>> fetchStats(
    DesktopConnection connection,
  ) async {
    final response = await client.requestJson(connection, 'GET', [
      'diff-stats',
    ]);
    return (response['stats'] as Map<String, dynamic>? ?? {}).map((id, value) {
      final json = value as Map<String, dynamic>;
      return MapEntry(
        id,
        TaskChangeStats(
          fileCount: (json['fileCount'] as num?)?.toInt() ?? 0,
          additions: (json['additions'] as num?)?.toInt() ?? 0,
          deletions: (json['deletions'] as num?)?.toInt() ?? 0,
        ),
      );
    });
  }
}
