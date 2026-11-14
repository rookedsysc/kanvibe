/// 데스크탑이 태스크 폴더에서 읽어 준 텍스트 파일
class TaskFile {
  const TaskFile({required this.path, required this.content});

  /// 태스크 폴더 기준 상대 경로
  final String path;
  final String content;

  factory TaskFile.fromJson(Map<String, dynamic> value) => TaskFile(
    path: value['path'] as String,
    content: value['content'] as String,
  );
}

/// 데스크탑이 파일을 내주지 않은 사유. 화면이 사유마다 다른 안내를 고른다
enum TaskFileFailure { outsideTaskFolder, notFound, tooLarge }

class TaskFileException implements Exception {
  const TaskFileException(this.failure);
  final TaskFileFailure failure;
}
