import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/features/board/data/board_snapshot.dart';
import 'package:kanvibe_mobile/src/features/board/domain/task_priority.dart';
import 'package:kanvibe_mobile/src/features/board/domain/task_status.dart';

void main() {
  group('상태 값', () {
    test('데스크탑이 쓰는 다섯 값을 모두 읽는다', () {
      for (final wireValue in [
        'todo',
        'progress',
        'pending',
        'review',
        'done',
      ]) {
        expect(
          TaskStatus.fromWire(wireValue),
          isNotNull,
          reason: '$wireValue 를 알아야 한다',
        );
      }
    });

    test('진행 중 상태의 값은 in_progress가 아니라 progress다', () {
      expect(TaskStatus.fromWire('in_progress'), isNull);
      expect(TaskStatus.fromWire('progress'), TaskStatus.progress);
    });

    test('칸 순서는 데스크탑 보드와 같다', () {
      expect(TaskStatus.values.map((status) => status.wireValue).toList(), [
        'todo',
        'progress',
        'pending',
        'review',
        'done',
      ]);
    });
  });

  group('보드 응답 해석', () {
    test('프로젝트 이름을 태스크에 붙여 준다', () {
      final snapshot = BoardSnapshot.fromJson({
        'projects': [
          {'id': 'p1', 'name': 'kanvibe'},
        ],
        'tasks': {
          'todo': [
            {'id': 't1', 'title': '제목', 'status': 'todo', 'projectId': 'p1'},
          ],
        },
      });

      expect(snapshot.tasksIn(TaskStatus.todo).single.projectName, 'kanvibe');
    });

    test('모르는 프로젝트를 가리켜도 태스크는 남는다', () {
      final snapshot = BoardSnapshot.fromJson({
        'projects': const [],
        'tasks': {
          'todo': [
            {'id': 't1', 'title': '제목', 'status': 'todo', 'projectId': 'gone'},
          ],
        },
      });

      expect(snapshot.tasksIn(TaskStatus.todo).single.projectName, isNull);
    });

    test('상태를 알 수 없는 항목은 버리고 나머지는 남긴다', () {
      final snapshot = BoardSnapshot.fromJson({
        'tasks': {
          'todo': [
            {'id': 't1', 'title': '살아남는다', 'status': 'todo'},
            {'id': 't2', 'title': '버려진다', 'status': '알 수 없음'},
          ],
        },
      });

      expect(snapshot.tasksIn(TaskStatus.todo).map((task) => task.id), ['t1']);
    });

    test('응답에 없는 상태는 빈 칸이 된다', () {
      final snapshot = BoardSnapshot.fromJson({'tasks': const {}});

      expect(snapshot.isEmpty, isTrue);
      expect(snapshot.tasksIn(TaskStatus.review), isEmpty);
    });

    test('원격 태스크는 호스트로 구분된다', () {
      final snapshot = BoardSnapshot.fromJson({
        'tasks': {
          'todo': [
            {'id': 't1', 'title': '제목', 'status': 'todo', 'sshHost': 'devbox'},
            {'id': 't2', 'title': '제목', 'status': 'todo'},
          ],
        },
      });

      final tasks = snapshot.tasksIn(TaskStatus.todo);
      expect(tasks[0].isRemote, isTrue);
      expect(tasks[1].isRemote, isFalse);
    });
  });

  group('정보 시트가 그리는 칸', () {
    test('데스크탑이 함께 보낸 설명과 베이스 브랜치, 우선순위를 읽는다', () {
      final snapshot = BoardSnapshot.fromJson({
        'projects': [
          {'id': 'p1', 'name': 'kanvibe'},
        ],
        'tasks': {
          'todo': [
            {
              'id': 't1',
              'title': '제목',
              'status': 'todo',
              'projectId': 'p1',
              'branchName': 'feat/x',
              'baseBranch': 'dev',
              'description': '무엇을 하는 태스크인지',
              'priority': 'high',
            },
          ],
        },
      });

      final task = snapshot.tasksIn(TaskStatus.todo).single;
      expect(task.baseBranch, 'dev');
      expect(task.description, '무엇을 하는 태스크인지');
      expect(task.priority, TaskPriority.high);
    });

    test('모르는 우선순위는 고르지 않은 것으로 본다', () {
      final snapshot = BoardSnapshot.fromJson({
        'tasks': {
          'todo': [
            {'id': 't1', 'title': '제목', 'status': 'todo', 'priority': 'urgent'},
          ],
        },
      });

      expect(snapshot.tasksIn(TaskStatus.todo).single.priority, isNull);
    });
  });

  group('생성 화면이 고를 프로젝트', () {
    test('데스크탑이 보낸 프로젝트를 기본 브랜치까지 들고 있는다', () {
      final snapshot = BoardSnapshot.fromJson({
        'projects': [
          {'id': 'p1', 'name': 'kanvibe', 'defaultBranch': 'dev'},
        ],
        'tasks': const {},
      });

      expect(snapshot.projects.single.defaultBranch, 'dev');
    });

    test('기본 브랜치가 안 오면 main으로 둔다', () {
      final snapshot = BoardSnapshot.fromJson({
        'projects': [
          {'id': 'p1', 'name': 'kanvibe'},
        ],
        'tasks': const {},
      });

      expect(snapshot.projects.single.defaultBranch, 'main');
    });

    test('worktree 프로젝트는 고를 대상에서 뺀다', () {
      final snapshot = BoardSnapshot.fromJson({
        'projects': [
          {'id': 'p1', 'name': 'kanvibe'},
          {'id': 'p2', 'name': 'kanvibe-worktree', 'isWorktree': true},
        ],
        'tasks': const {},
      });

      expect(snapshot.projects, hasLength(2));
      expect(snapshot.creatableProjects.single.id, 'p1');
    });
  });
}
