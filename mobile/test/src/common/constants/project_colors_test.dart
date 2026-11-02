import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/constants/project_colors.dart';
import 'package:kanvibe_mobile/src/features/board/data/board_snapshot.dart';
import 'package:kanvibe_mobile/src/features/board/domain/task_status.dart';

void main() {
  test('프로젝트 지정 색상을 상태 변경 후에도 유지한다', () {
    final snapshot = BoardSnapshot.fromJson({
      'projects': [
        {'id': 'p', 'name': 'Project', 'color': '#123456'},
      ],
      'tasks': {
        'todo': [
          {'id': 't', 'title': 'Task', 'status': 'todo', 'projectId': 'p'},
        ],
      },
    });
    final task = snapshot.tasksIn(TaskStatus.todo).single;
    expect(task.projectColor, '#123456');
    expect(task.withStatus(TaskStatus.done).projectColor, '#123456');
    expect(
      projectColor(task.projectName!, task.projectColor),
      const Color(0xFF123456),
    );
  });
  test('데스크탑 기본 팔레트와 밝고 어두운 배경의 대비를 따른다', () {
    expect(projectColor('A', null), const Color(0xFF93C5FD));
    expect(projectColor('A', 'invalid'), const Color(0xFF93C5FD));
    expect(projectTextColor(Colors.white), const Color(0xFF111827));
    expect(projectTextColor(Colors.black), Colors.white);
  });
}
