/// 데스크탑에 등록된 프로젝트 하나.
///
/// 보드 필터와 태스크 생성 화면이 같은 프로젝트 정보를 쓴다.
class BoardProject {
  const BoardProject({
    required this.id,
    required this.name,
    required this.defaultBranch,
    required this.isWorktree,
    this.color,
    this.repoPath,
    this.sshHost,
    this.iconDataUrl,
  });

  final String id;
  final String name;
  final String? color;
  final String? repoPath;
  final String? sshHost;
  final String? iconDataUrl;

  /// 베이스 브랜치 칸의 처음 값. 데스크탑 생성 화면도 프로젝트를 고르면 이 값을 먼저 넣는다
  final String defaultBranch;

  /// worktree로 만들어진 프로젝트. 여기서 또 worktree를 치면 안 되므로 생성 화면의 후보에서 뺀다
  final bool isWorktree;

  bool matchesSearch(String query) {
    final normalizedQuery = query.trim().toLowerCase();
    if (normalizedQuery.isEmpty) return true;
    return [name, repoPath ?? '', sshHost ?? '로컬'].any((value) {
      final target = value.toLowerCase();
      var offset = 0;
      for (final character in normalizedQuery.split('')) {
        final index = target.indexOf(character, offset);
        if (index < 0) return false;
        offset = index + 1;
      }
      return true;
    });
  }

  static BoardProject? fromJson(Map<String, dynamic> json) {
    final id = json['id'];
    final name = json['name'];

    if (id is! String || name is! String) {
      return null;
    }

    return BoardProject(
      id: id,
      name: name,
      color: json['color'] is String ? json['color'] as String : null,
      repoPath: _readString(json['repoPath']),
      sshHost: _readString(json['sshHost']),
      iconDataUrl: _readString(json['iconDataUrl']),
      defaultBranch: json['defaultBranch'] is String
          ? json['defaultBranch'] as String
          : 'main',
      isWorktree: json['isWorktree'] == true,
    );
  }

  static String? _readString(Object? value) =>
      value is String && value.isNotEmpty ? value : null;
}
