import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_command.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/board/application/task_deletion.dart';

import '../../../support/fake_desktop.dart';

Map<String, dynamic> boardWithTodo(List<Map<String, dynamic>> todo) => {
  'projects': [
    {'id': 'p1', 'name': 'kanvibe'},
  ],
  'tasks': {
    'todo': todo,
    'progress': [],
    'pending': [],
    'review': [],
    'done': [],
  },
};

const _todoTask = {'id': 't1', 'title': '할 일 하나', 'status': 'todo'};

/// 보드를 구독해 둔 container. 삭제가 보드를 다시 읽게 하는지 보려면 누군가 보드를 보고 있어야 한다
Future<ProviderContainer> containerWatchingBoard(
  FakeDesktopClient client,
) async {
  final container = ProviderContainer(
    overrides: [
      desktopClientProvider.overrideWithValue(client),
      connectionStoreProvider.overrideWithValue(FakeConnectionStore()),
    ],
  );
  addTearDown(container.dispose);
  container.listen(boardProvider, (_, _) {});
  await container.read(boardProvider.future);
  return container;
}

void main() {
  test('같은 태스크의 삭제를 겹쳐 요청하면 명령은 한 번만 보내고 끝나면 보드를 다시 읽는다', () async {
    final pending = Completer<Map<String, dynamic>>();
    final client = _HeldDeletionClient(
      board: boardWithTodo([_todoTask]),
      pending: pending,
    );
    final container = await containerWatchingBoard(client);
    final controller = container.read(taskDeletionProvider.notifier);
    final boardCallsBefore = client.boardCalls.length;

    final first = controller.delete('t1', isForced: false);
    final second = await controller.delete('t1', isForced: false);

    expect(second, isNull);
    expect(client.deleteRequestCount, 1);
    expect(
      container.read(taskDeletionProvider)['t1'],
      isA<TaskDeletionRunning>(),
    );

    pending.complete({'ok': true, 'failedPeerCount': 0});
    final outcome = await first;
    await container.read(boardProvider.future);

    expect(outcome, isA<TaskDeletionSucceeded>());
    expect(container.read(taskDeletionProvider)['t1'], isA<TaskDeletionDone>());
    expect(client.boardCalls.length, greaterThan(boardCallsBefore));
  });

  test('실패하면 진행 상태를 지워 다시 시도할 수 있게 한다', () async {
    final client = FakeDesktopClient(
      board: boardWithTodo([_todoTask]),
      deleteErrors: const [DesktopCommandFailure('server-error', '서버 오류')],
    );
    final container = await containerWatchingBoard(client);
    final controller = container.read(taskDeletionProvider.notifier);

    final outcome = await controller.delete('t1', isForced: false);

    expect(outcome, isA<TaskDeletionFailed>());
    expect(container.read(taskDeletionProvider)['t1'], isNull);
    expect(
      await controller.delete('t1', isForced: false),
      isA<TaskDeletionSucceeded>(),
    );
  });

  group('응답을 기다리다 시간이 넘으면', () {
    test('보드를 다시 읽어 태스크가 없으면 지워진 것으로 본다', () async {
      final board = boardWithTodo([_todoTask]);
      final client = _TimedOutDeletionClient(board: board);
      final container = await containerWatchingBoard(client);
      (board['tasks'] as Map<String, dynamic>)['todo'] =
          <Map<String, dynamic>>[];

      final outcome = await container
          .read(taskDeletionProvider.notifier)
          .delete('t1', isForced: false);

      expect(outcome, isA<TaskDeletionSucceeded>());
    });

    test('보드에 태스크가 남아 있으면 결과를 확인하지 못했다고 알린다', () async {
      final client = _TimedOutDeletionClient(board: boardWithTodo([_todoTask]));
      final container = await containerWatchingBoard(client);

      final outcome = await container
          .read(taskDeletionProvider.notifier)
          .delete('t1', isForced: false);

      expect(outcome, isA<TaskDeletionUnconfirmed>());
      expect(
        (outcome! as TaskDeletionFailed).message,
        '삭제 결과를 확인하지 못했습니다. 잠시 뒤 보드에서 다시 확인해 주세요.',
      );
      expect(container.read(taskDeletionProvider)['t1'], isNull);
    });
  });
}

class _HeldDeletionClient extends FakeDesktopClient {
  _HeldDeletionClient({required super.board, required this.pending});

  final Completer<Map<String, dynamic>> pending;
  int deleteRequestCount = 0;

  @override
  Future<Map<String, dynamic>> runCommand(
    DesktopConnection connection,
    DesktopCommand command, {
    Duration? timeout,
  }) {
    deleteRequestCount++;
    return pending.future;
  }
}

class _TimedOutDeletionClient extends FakeDesktopClient {
  _TimedOutDeletionClient({required super.board});

  @override
  Future<Map<String, dynamic>> runCommand(
    DesktopConnection connection,
    DesktopCommand command, {
    Duration? timeout,
  }) => Future.error(TimeoutException('응답 없음'));
}
