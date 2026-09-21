/// 데스크탑에 등록된 프로젝트 하나.
///
/// 보드는 태스크에 이름을 붙이는 데만 쓰지만, 태스크 생성 화면은 고를 대상 자체로 쓴다.
/// 그래서 이름만 뽑아 두지 않고 데스크탑이 보낸 칸을 그대로 들고 있는다.
class BoardProject {
  const BoardProject({
    required this.id,
    required this.name,
    required this.defaultBranch,
    required this.isWorktree,
  });

  final String id;
  final String name;

  /// 베이스 브랜치 칸의 처음 값. 데스크탑 생성 화면도 프로젝트를 고르면 이 값을 먼저 넣는다
  final String defaultBranch;

  /// worktree로 만들어진 프로젝트. 여기서 또 worktree를 치면 안 되므로 생성 화면의 후보에서 뺀다
  final bool isWorktree;

  static BoardProject? fromJson(Map<String, dynamic> json) {
    final id = json['id'];
    final name = json['name'];

    if (id is! String || name is! String) {
      return null;
    }

    return BoardProject(
      id: id,
      name: name,
      defaultBranch: json['defaultBranch'] is String
          ? json['defaultBranch'] as String
          : 'main',
      isWorktree: json['isWorktree'] == true,
    );
  }
}
