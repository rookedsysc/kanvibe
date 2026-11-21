import 'task_priority.dart';
import 'task_status.dart';

/// 보드에 놓이는 태스크 하나.
///
/// 데스크탑 `KanbanTask`의 모든 칸을 가져오지 않는다. 목록 카드·상세 정보·태스크 생성에 필요한 칸만 담는다.
class BoardTask {
  const BoardTask({
    required this.id,
    required this.title,
    required this.status,
    this.branchName,
    this.baseBranch,
    this.projectId,
    this.projectName,
    this.projectColor,
    this.description,
    this.priority,
    this.agentType,
    this.sshHost,
    this.prUrl,
    this.createdAt,
    this.updatedAt,
    this.sessionType,
    this.worktreePath,
    this.projectIconDataUrl,
  });

  final String id;
  final String title;
  final TaskStatus status;
  final String? branchName;
  final String? baseBranch;
  final String? projectId;
  final String? projectName;
  final String? projectColor;
  final String? description;
  final TaskPriority? priority;
  final String? agentType;
  final String? sshHost;
  final String? prUrl;
  final DateTime? createdAt;
  final DateTime? updatedAt;
  final String? sessionType;
  final String? worktreePath;
  final String? projectIconDataUrl;

  /// 태스크가 원격 호스트에서 도는지. 목록에서 배지로 구분해 준다
  bool get isRemote => sshHost != null && sshHost!.isNotEmpty;

  /// 상태만 바꾼 같은 태스크.
  ///
  /// 상태를 옮기고 나면 상세 화면이 든 태스크도 새 상태여야 다음에 열 상태 시트가 옳은 후보를 낸다.
  /// 보드를 다시 불러 기다리지 않고 여기서 바로 갈아 끼운다.
  BoardTask withStatus(TaskStatus status) => BoardTask(
    id: id,
    title: title,
    status: status,
    branchName: branchName,
    baseBranch: baseBranch,
    projectId: projectId,
    projectName: projectName,
    projectColor: projectColor,
    description: description,
    priority: priority,
    agentType: agentType,
    sshHost: sshHost,
    prUrl: prUrl,
    createdAt: createdAt,
    updatedAt: updatedAt,
    sessionType: sessionType,
    worktreePath: worktreePath,
    projectIconDataUrl: projectIconDataUrl,
  );

  static BoardTask? fromJson(
    Map<String, dynamic> json,
    Map<String, String> projectNamesById, {
    Map<String, String> projectColorsById = const {},
    Map<String, String> projectIconsById = const {},
  }) {
    final id = json['id'];
    final title = json['title'];
    final status = TaskStatus.fromWire(json['status']?.toString() ?? '');

    if (id is! String || title is! String || status == null) {
      return null;
    }

    final projectId = json['projectId'];
    final project = json['project'];
    return BoardTask(
      id: id,
      title: title,
      status: status,
      branchName: _readString(json['branchName']),
      baseBranch: _readString(json['baseBranch']),
      projectId: _readString(projectId),
      projectName:
          (projectId is String ? projectNamesById[projectId] : null) ??
          _readString(json['projectName']),
      projectColor:
          (projectId is String ? projectColorsById[projectId] : null) ??
          _readString(json['projectColor']) ??
          (projectId is String && project is Map && project['id'] == projectId
              ? _readString(project['color'])
              : null),
      description: _readString(json['description']),
      priority: TaskPriority.fromWire(_readString(json['priority'])),
      agentType: _readString(json['agentType']),
      sshHost: _readString(json['sshHost']),
      prUrl: _readString(json['prUrl']),
      createdAt: DateTime.tryParse(json['createdAt']?.toString() ?? ''),
      updatedAt: DateTime.tryParse(json['updatedAt']?.toString() ?? ''),
      sessionType: _readString(json['sessionType']),
      worktreePath: _readString(json['worktreePath']),
      projectIconDataUrl:
          (projectId is String ? projectIconsById[projectId] : null) ??
          _readString(json['projectIconDataUrl']),
    );
  }

  static String? _readString(Object? value) =>
      value is String && value.isNotEmpty ? value : null;
}
