import 'board_project.dart';
import 'board_task.dart';
import 'task_priority.dart';

enum BoardTaskKind { all, project, task }

enum BoardSortField { priority, createdAt, updatedAt, title, project }

class BoardSortKey {
  const BoardSortKey(this.field, {this.descending = false});
  final BoardSortField field;
  final bool descending;

  Map<String, String> toJson() => {
    'field': field.name,
    'direction': descending ? 'desc' : 'asc',
  };
}

bool isProjectRoot(BoardTask task, List<BoardProject> projects) => projects.any(
  (project) =>
      project.id == task.projectId &&
      !project.isWorktree &&
      task.branchName != null &&
      project.defaultBranch == task.branchName,
);

TaskPriority? effectivePriority(
  BoardTask task,
  List<BoardTask> tasks,
  List<BoardProject> projects,
) {
  if (task.priority != null) return task.priority;
  return tasks
      .where(
        (root) =>
            root.projectId == task.projectId && isProjectRoot(root, projects),
      )
      .firstOrNull
      ?.priority;
}

List<BoardTask> sortBoardTasks(
  List<BoardTask> tasks,
  List<BoardSortKey> keys,
  List<BoardTask> allTasks,
  List<BoardProject> projects,
) {
  if (keys.isEmpty) return tasks;
  Object? value(BoardTask task, BoardSortField field) => switch (field) {
    BoardSortField.priority => switch (effectivePriority(
      task,
      allTasks,
      projects,
    )) {
      TaskPriority.high => 0,
      TaskPriority.medium => 1,
      TaskPriority.low => 2,
      null => null,
    },
    BoardSortField.createdAt => task.createdAt?.millisecondsSinceEpoch,
    BoardSortField.updatedAt => task.updatedAt?.millisecondsSinceEpoch,
    BoardSortField.title => task.title.toLowerCase(),
    BoardSortField.project => task.projectName?.toLowerCase(),
  };
  final sorted = [...tasks];
  sorted.sort((left, right) {
    for (final key in keys) {
      final a = value(left, key.field), b = value(right, key.field);
      if (a == null || b == null) {
        if (a == b) continue;
        return a == null ? 1 : -1;
      }
      final comparison = a is int && b is int
          ? a.compareTo(b)
          : a.toString().compareTo(b.toString());
      if (comparison != 0) return key.descending ? -comparison : comparison;
    }
    final comparison = (right.updatedAt?.millisecondsSinceEpoch ?? 0).compareTo(
      left.updatedAt?.millisecondsSinceEpoch ?? 0,
    );
    return comparison != 0 ? comparison : left.id.compareTo(right.id);
  });
  return sorted;
}

int? fuzzyTaskScore(BoardTask task, String query) {
  final needle = query.trim().toLowerCase();
  if (needle.isEmpty) return 0;
  int? best;
  for (final field in [
    task.title,
    task.projectName ?? '',
    task.branchName ?? '',
  ]) {
    final text = field.toLowerCase();
    var cursor = 0;
    var score = 0;
    var matched = true;
    for (final character in needle.split('')) {
      final found = text.indexOf(character, cursor);
      if (found < 0) {
        matched = false;
        break;
      }
      score += found - cursor;
      cursor = found + 1;
    }
    if (matched && (best == null || score < best)) best = score;
  }
  return best;
}
