import '../domain/board_task.dart';
import '../domain/task_status.dart';

/// 한 번의 조회로 받은 보드 전체.
///
/// 데스크탑과 같은 조회를 쓰므로 정렬(최근 수정 순)과 done 페이지 크기가 데스크탑과 같다.
class BoardSnapshot {
  const BoardSnapshot({required this.tasksByStatus});

  final Map<TaskStatus, List<BoardTask>> tasksByStatus;

  List<BoardTask> tasksIn(TaskStatus status) =>
      tasksByStatus[status] ?? const [];

  bool get isEmpty => tasksByStatus.values.every((tasks) => tasks.isEmpty);

  /// 데스크탑 `getTasksByStatus()` 응답을 화면이 쓰는 모양으로 바꾼다.
  /// 프로젝트 이름은 별도 목록으로 오므로 여기서 태스크에 붙여 준다.
  static BoardSnapshot fromJson(Map<String, dynamic> json) {
    final projectNamesById = <String, String>{};
    for (final project in _readList(
      json['projects'],
    ).whereType<Map<String, dynamic>>()) {
      final id = project['id'];
      final name = project['name'];
      if (id is String && name is String) {
        projectNamesById[id] = name;
      }
    }

    final rawTasks = json['tasks'];
    final tasksByStatus = <TaskStatus, List<BoardTask>>{};

    for (final status in TaskStatus.values) {
      final entries = rawTasks is Map
          ? _readList(rawTasks[status.wireValue])
          : const [];
      tasksByStatus[status] = entries
          .whereType<Map<String, dynamic>>()
          .map((entry) => BoardTask.fromJson(entry, projectNamesById))
          .whereType<BoardTask>()
          .toList();
    }

    return BoardSnapshot(tasksByStatus: tasksByStatus);
  }

  /// 목록이 아닌 값이 오면 그 칸만 비운다. 응답 한 곳이 어긋났다고 보드 전체를 못 그리면 안 된다
  static List<dynamic> _readList(Object? value) =>
      value is List<dynamic> ? value : const [];
}
