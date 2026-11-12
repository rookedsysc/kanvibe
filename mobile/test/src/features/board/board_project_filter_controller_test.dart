import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/board/application/board_project_filter_controller.dart';
import 'package:kanvibe_mobile/src/features/board/application/board_desktop_filter_controller.dart';
import 'package:kanvibe_mobile/src/features/board/domain/task_status.dart';

import '../../../support/fake_desktop.dart';

void main() {
  test('원격 orphan 태스크를 프로젝트 필터로 제외해도 호스트 선택은 유지한다', () async {
    final container = ProviderContainer.test(
      overrides: [
        desktopClientProvider.overrideWithValue(
          FakeDesktopClient(
            board: {
              'projects': [
                {'id': 'local', 'name': 'Local'},
              ],
              'tasks': {
                'todo': [
                  {
                    'id': 'local-task',
                    'title': 'Local',
                    'projectId': 'local',
                    'status': 'todo',
                  },
                  {
                    'id': 'remote-task',
                    'title': 'Remote',
                    'sshHost': 'devbox',
                    'status': 'todo',
                  },
                ],
              },
            },
          ),
        ),
        connectionStoreProvider.overrideWithValue(FakeConnectionStore()),
      ],
    );
    await container.read(boardProvider.future);
    container
        .read(boardDesktopFilterProvider('127.0.0.1:9736').notifier)
        .toggleDesktop('ssh:devbox');
    container
        .read(boardProjectFilterProvider('127.0.0.1:9736').notifier)
        .toggleProject('local');
    expect(
      container
          .read(filteredBoardProvider)
          .requireValue
          .tasksIn(TaskStatus.todo),
      isEmpty,
    );
  });

  test('데스크톱 필터 선택은 연결한 PC별로 분리하고 전체로 복원한다', () {
    final container = ProviderContainer.test();
    container
        .read(boardDesktopFilterProvider('first').notifier)
        .toggleDesktop('ssh:devbox');
    container
        .read(boardDesktopFilterProvider('second').notifier)
        .toggleDesktop('local');
    expect(container.read(boardDesktopFilterProvider('first')), {'ssh:devbox'});
    container
        .read(boardDesktopFilterProvider('first').notifier)
        .toggleDesktop('local');
    expect(container.read(boardDesktopFilterProvider('first')), {
      'ssh:devbox',
      'local',
    });
    container
        .read(boardDesktopFilterProvider('first').notifier)
        .toggleDesktop('ssh:devbox');
    expect(container.read(boardDesktopFilterProvider('first')), {'local'});
    expect(container.read(boardDesktopFilterProvider('second')), {'local'});
    container
        .read(boardDesktopFilterProvider('first').notifier)
        .showAllDesktops();
    expect(container.read(boardDesktopFilterProvider('first')), isEmpty);
    expect(container.read(boardDesktopFilterProvider('second')), {'local'});
  });

  test('PC별 복수 선택과 전체 해제는 다른 PC 선택을 바꾸지 않는다', () {
    final container = ProviderContainer.test();
    final first = container.read(boardProjectFilterProvider('first').notifier);
    final second = container.read(
      boardProjectFilterProvider('second').notifier,
    );

    first.toggleProject('one');
    first.toggleProject('two');
    second.toggleProject('three');
    expect(container.read(boardProjectFilterProvider('first')), {'one', 'two'});
    expect(container.read(boardProjectFilterProvider('second')), {'three'});

    first.toggleProject('one');
    expect(container.read(boardProjectFilterProvider('first')), {'two'});
    first.showAllProjects();
    expect(container.read(boardProjectFilterProvider('first')), isEmpty);
    expect(container.read(boardProjectFilterProvider('second')), {'three'});
  });

  test('보드 소비자가 잠시 없어져도 같은 PC의 선택은 세션 동안 남는다', () async {
    final container = ProviderContainer.test();
    final subscription = container.listen(
      boardProjectFilterProvider('pc'),
      (_, _) {},
    );
    container
        .read(boardProjectFilterProvider('pc').notifier)
        .toggleProject('one');
    subscription.close();
    await container.pump();

    expect(container.read(boardProjectFilterProvider('pc')), {'one'});
  });

  test('선택은 원본 보드를 변경하지 않고 조회 갱신 후 사라진 ID는 전체로 본다', () async {
    final projects = <Map<String, dynamic>>[
      {'id': 'one', 'name': 'One', 'repoPath': '/one'},
      {'id': 'two', 'name': 'Two', 'repoPath': '/two'},
    ];
    final response = <String, dynamic>{
      'projects': projects,
      'tasks': {
        'todo': [
          {'id': 't-one', 'title': 'One', 'status': 'todo', 'projectId': 'one'},
          {'id': 't-two', 'title': 'Two', 'status': 'todo', 'projectId': 'two'},
        ],
      },
    };
    final container = ProviderContainer.test(
      overrides: [
        desktopClientProvider.overrideWithValue(
          FakeDesktopClient(board: response),
        ),
        connectionStoreProvider.overrideWithValue(FakeConnectionStore()),
      ],
    );
    await container.read(boardProvider.future);
    container
        .read(boardProjectFilterProvider('127.0.0.1:9736').notifier)
        .toggleProject('one');

    expect(
      container
          .read(filteredBoardProvider)
          .requireValue
          .tasksIn(TaskStatus.todo)
          .map((task) => task.id),
      ['t-one'],
    );
    expect(
      container.read(boardProvider).requireValue.tasksIn(TaskStatus.todo),
      hasLength(2),
    );
    projects.removeWhere((project) => project['id'] == 'one');
    container.invalidate(boardProvider);
    await container.read(boardProvider.future);

    expect(
      container
          .read(filteredBoardProvider)
          .requireValue
          .tasksIn(TaskStatus.todo)
          .map((task) => task.id),
      ['t-one', 't-two'],
    );
  });
}
