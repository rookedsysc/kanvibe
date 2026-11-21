class ChangedFile {
  const ChangedFile({
    required this.path,
    required this.status,
    this.originalPath,
    this.additions = 0,
    this.deletions = 0,
  });
  final String path;
  final String? originalPath;
  final String status;
  final int additions;
  final int deletions;
  String get statusLetter => switch (status) {
    'added' => 'A',
    'deleted' => 'D',
    'renamed' => 'R',
    _ => 'M',
  };
}

class DiffFileContents {
  const DiffFileContents({required this.original, required this.content});
  final String original;
  final String content;
}

class TaskChangeStats {
  const TaskChangeStats({
    required this.fileCount,
    required this.additions,
    required this.deletions,
  });
  final int fileCount;
  final int additions;
  final int deletions;
}
