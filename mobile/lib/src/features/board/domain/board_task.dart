import 'task_status.dart';

/// 보드에 놓이는 태스크 하나.
///
/// 데스크탑 `KanbanTask`의 모든 칸을 가져오지 않는다. 모바일은 읽기만 하므로 목록과 상세에 실제로 그려지는 것만 담는다.
class BoardTask {
  const BoardTask({
    required this.id,
    required this.title,
    required this.status,
    this.branchName,
    this.projectName,
    this.agentType,
    this.sshHost,
    this.prUrl,
  });

  final String id;
  final String title;
  final TaskStatus status;
  final String? branchName;
  final String? projectName;
  final String? agentType;
  final String? sshHost;
  final String? prUrl;

  /// 태스크가 원격 호스트에서 도는지. 목록에서 배지로 구분해 준다
  bool get isRemote => sshHost != null && sshHost!.isNotEmpty;

  static BoardTask? fromJson(
    Map<String, dynamic> json,
    Map<String, String> projectNamesById,
  ) {
    final id = json['id'];
    final title = json['title'];
    final status = TaskStatus.fromWire(json['status']?.toString() ?? '');

    if (id is! String || title is! String || status == null) {
      return null;
    }

    final projectId = json['projectId'];
    return BoardTask(
      id: id,
      title: title,
      status: status,
      branchName: _readString(json['branchName']),
      projectName: projectId is String ? projectNamesById[projectId] : null,
      agentType: _readString(json['agentType']),
      sshHost: _readString(json['sshHost']),
      prUrl: _readString(json['prUrl']),
    );
  }

  static String? _readString(Object? value) =>
      value is String && value.isNotEmpty ? value : null;
}
