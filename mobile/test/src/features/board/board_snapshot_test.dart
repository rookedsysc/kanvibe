import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/features/board/data/board_snapshot.dart';
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
}
