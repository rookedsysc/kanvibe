import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../../board/domain/board_task.dart';

class TaskDetailsApi {
  TaskDetailsApi(this.client, this.connection);
  final DesktopClient client;
  final DesktopConnection connection;

  BoardTask parse(Map<String, dynamic> json) {
    final project = json['project'];
    final normalized = {
      ...json,
      if (project is Map) 'projectName': project['name'],
    };
    final task = BoardTask.fromJson(
      normalized,
      const {},
      projectColorsById: {
        if (project is Map &&
            project['id'] is String &&
            project['color'] is String)
          project['id'] as String: project['color'] as String,
      },
    );
    if (task == null) throw const FormatException('태스크 정보를 읽지 못했습니다.');
    return task;
  }

  Future<BoardTask> read(String taskId) async {
    final json = await client.fetchTask(connection, taskId);
    if (json == null) throw DesktopRequestException(404);
    return parse(json);
  }

  Future<BoardTask> update(
    String taskId, {
    required String description,
    String? priority,
  }) async {
    final response = await client.requestJson(
      connection,
      'PATCH',
      ['tasks', taskId, 'details'],
      body: {'description': description, 'priority': priority},
    );
    if (response['task'] is! Map) {
      throw const FormatException('저장 응답을 읽지 못했습니다.');
    }
    return read(taskId);
  }

  Future<void> setColor(String projectId, String color) async {
    await client.requestJson(
      connection,
      'PATCH',
      ['projects', projectId, 'color'],
      body: {'color': color},
    );
  }

  Future<List<BoardTask>> related(String projectId) async {
    final response = await client.requestJson(connection, 'GET', [
      'projects',
      projectId,
      'tasks',
    ]);
    return (response['tasks'] as List)
        .map((json) => parse(Map<String, dynamic>.from(json as Map)))
        .toList();
  }

  Future<BoardTask?> baseTask(String projectId, String branch) async {
    final response = await client.requestJson(
      connection,
      'GET',
      ['projects', projectId, 'task'],
      query: {'branch': branch},
    );
    final json = response['task'];
    return json is Map<String, dynamic> ? parse(json) : null;
  }

  Future<String?> findPullRequest(String taskId) async {
    final response = await client.requestJson(connection, 'POST', [
      'tasks',
      taskId,
      'pr',
      'refresh',
    ]);
    return response['prUrl'] as String?;
  }

  Future<bool> isDoneNoticeDismissed() async {
    final response = await client.requestJson(connection, 'GET', ['settings']);
    return (response['settings'] as Map)['doneAlertDismissed'] == true;
  }

  Future<void> dismissDoneNotice() async {
    await client.requestJson(
      connection,
      'PATCH',
      ['settings'],
      body: {'doneAlertDismissed': true},
    );
  }
}
