import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:kanvibe_mobile/src/features/notifications/application/notification_inbox_controller.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_command.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/board/presentation/board_screen.dart';
import 'package:kanvibe_mobile/src/features/connection/presentation/desktop_connections_screen.dart';

import '../../../support/fake_desktop.dart';
import '../../../support/fake_notification_inbox.dart';

Map<String, dynamic> boardWith({
  List<Map<String, dynamic>> progress = const [],
}) => {
  'projects': [
    {'id': 'p1', 'name': 'kanvibe'},
  ],
  'tasks': {
    'todo': [
      {'id': 't1', 'title': '할 일 하나', 'status': 'todo', 'projectId': 'p1'},
    ],
    'progress': progress,
    'pending': [],
    'review': [],
    'done': [],
  },
};

void main() {
  for (final size in [phoneSize, tabletSize]) {
    testWidgets('태스크별 미읽음 알림 수를 표시한다: $size', (tester) async {
      await tester.binding.setSurfaceSize(size);
      addTearDown(() => tester.binding.setSurfaceSize(null));
      final client = InboxDesktopClient(
        board: boardWith(
          progress: [
            {'id': 't2', 'title': '알림 없는 태스크', 'status': 'progress'},
          ],
        ),
      );
      client.notificationsByConnection['127.0.0.1:9736'] = [
        inboxNotice(taskId: 't1'),
        inboxNotice(id: 'n2', taskId: 't1'),
        inboxNotice(id: 'read', taskId: 't1', isRead: true),
        inboxNotice(id: 'read-t2', taskId: 't2', isRead: true),
        inboxNotice(id: 'no-task', taskId: null),
        inboxNotice(id: 'missing-task', taskId: 'missing'),
      ];
      await tester.pumpWidget(
        wrapWithApp(const BoardScreen(), client: client, size: size),
      );
      await tester.pumpAndSettle();

      final indicator = find.byKey(const ValueKey('task-unread-t1'));
      expect(indicator, findsOneWidget);
      expect(
        find.descendant(of: indicator, matching: find.text('2')),
        findsOneWidget,
      );
      expect(find.byKey(const ValueKey('task-unread-t2')), findsNothing);
      expect(
        tester.widget<Semantics>(indicator).properties.label,
        '읽지 않은 알림 2개',
      );
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('알림 갱신과 개별·전체 읽음 처리가 태스크 배지에 반영된다', (tester) async {
    await tester.binding.setSurfaceSize(phoneSize);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final client = InboxDesktopClient(board: boardWith());
    client.notificationsByConnection['127.0.0.1:9736'] = [
      inboxNotice(taskId: 't1'),
    ];
    await tester.pumpWidget(
      wrapWithApp(const BoardScreen(), client: client, size: phoneSize),
    );
    await tester.pumpAndSettle();
    final container = ProviderScope.containerOf(
      tester.element(find.byType(BoardScreen)),
    );
    final indicator = find.byKey(const ValueKey('task-unread-t1'));
    expect(
      find.descendant(of: indicator, matching: find.text('1')),
      findsOneWidget,
    );

    client.notificationsByConnection['127.0.0.1:9736']!.add(
      inboxNotice(id: 'n2', taskId: 't1'),
    );
    container.invalidate(notificationInboxProvider);
    await tester.pumpAndSettle();
    expect(
      find.descendant(of: indicator, matching: find.text('2')),
      findsOneWidget,
    );

    await container.read(notificationInboxProvider.notifier).markRead('n1');
    await tester.pumpAndSettle();
    expect(
      find.descendant(of: indicator, matching: find.text('1')),
      findsOneWidget,
    );
    await container.read(notificationInboxProvider.notifier).markAllRead();
    await tester.pumpAndSettle();
    expect(indicator, findsNothing);
  });

  testWidgets('알림 조회 실패가 태스크 목록을 가리지 않는다', (tester) async {
    await tester.binding.setSurfaceSize(phoneSize);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final client = InboxDesktopClient(board: boardWith());
    client.onFetch = (_) async => throw StateError('알림 조회 실패');
    await tester.pumpWidget(
      wrapWithApp(const BoardScreen(), client: client, size: phoneSize),
    );
    await tester.pumpAndSettle();
    expect(find.text('할 일 하나'), findsOneWidget);
    expect(find.byKey(const ValueKey('task-unread-t1')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('당겨서 새로고침은 보드를 다시 받아 올 때까지 끝나지 않는다', (tester) async {
    await tester.binding.setSurfaceSize(phoneSize);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final client = FakeDesktopClient(board: boardWith());
    await tester.pumpWidget(
      wrapWithApp(const BoardScreen(), client: client, size: phoneSize),
    );
    await tester.pumpAndSettle();

    final heldBoard = Completer<Map<String, dynamic>>();
    client.onFetchBoard = () => heldBoard.future;
    var isRefreshDone = false;
    unawaited(
      tester
          .state<RefreshIndicatorState>(find.byType(RefreshIndicator))
          .show()
          .then((_) => isRefreshDone = true),
    );
    for (var frame = 0; frame < 10; frame++) {
      await tester.pump(const Duration(milliseconds: 300));
    }

    expect(isRefreshDone, isFalse);
    expect(find.byType(RefreshProgressIndicator), findsOneWidget);

    heldBoard.complete(boardWith());
    await tester.pumpAndSettle();

    expect(isRefreshDone, isTrue);
    expect(find.text('할 일 하나'), findsOneWidget);
  });

  testWidgets('새로고침이 실패하면 이전 카드를 그대로 두고 다시 시도를 안내한다', (tester) async {
    await tester.binding.setSurfaceSize(phoneSize);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final client = FakeDesktopClient(board: boardWith());
    await tester.pumpWidget(
      wrapWithApp(const BoardScreen(), client: client, size: phoneSize),
    );
    await tester.pumpAndSettle();

    client.boardError = const DesktopRequestException(503);
    unawaited(
      tester.state<RefreshIndicatorState>(find.byType(RefreshIndicator)).show(),
    );
    await tester.pumpAndSettle();

    expect(find.text('할 일 하나'), findsOneWidget);
    expect(find.text('보드를 새로 고치지 못했습니다. 이전 내용을 보여 줍니다.'), findsOneWidget);

    client.boardError = null;
    final callsBeforeRetry = client.boardCalls.length;
    await tester.tap(find.widgetWithText(SnackBarAction, '다시 시도'));
    await tester.pumpAndSettle();

    expect(client.boardCalls.length, callsBeforeRetry + 1);
  });

  testWidgets('당겨서 새로고침이 아닌 갱신이 실패해도 이전 카드 위에 갱신 실패를 알리고 다시 시도할 수 있다', (
    tester,
  ) async {
    await tester.binding.setSurfaceSize(phoneSize);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final client = FakeDesktopClient(board: boardWith());
    await tester.pumpWidget(
      wrapWithApp(const BoardScreen(), client: client, size: phoneSize),
    );
    await tester.pumpAndSettle();

    // 삭제·생성 뒤처럼 화면 밖에서 보드를 다시 읽다 실패한 경우
    client.boardError = const DesktopRequestException(503);
    ProviderScope.containerOf(tester.element(find.byType(BoardScreen)))
        .invalidate(boardProvider);
    await tester.pumpAndSettle();

    expect(find.text('할 일 하나'), findsOneWidget);
    expect(find.text('최신 보드를 불러오지 못해 이전 내용을 보여 줍니다.'), findsOneWidget);

    client.boardError = null;
    await tester.tap(find.widgetWithText(TextButton, '다시 시도'));
    await tester.pumpAndSettle();

    expect(find.text('최신 보드를 불러오지 못해 이전 내용을 보여 줍니다.'), findsNothing);
  });

  testWidgets('홈 상단은 알림과 더보기로 줄이고 등록과 새로고침을 메뉴에 둔다', (tester) async {
    await tester.binding.setSurfaceSize(phoneSize);
    final client = FakeDesktopClient(board: boardWith());
    await tester.pumpWidget(
      wrapWithApp(const BoardScreen(), client: client, size: phoneSize),
    );
    await tester.pumpAndSettle();
    final appBar = find.byType(AppBar);
    expect(
      find.descendant(of: appBar, matching: find.byTooltip('프로젝트 필터')),
      findsNothing,
    );
    expect(
      find.descendant(of: appBar, matching: find.byTooltip('새로고침')),
      findsNothing,
    );
    expect(find.byTooltip('프로젝트 필터'), findsOneWidget);
    await tester.tap(find.byTooltip('보드 메뉴'));
    await tester.pumpAndSettle();
    expect(find.text('프로젝트 등록'), findsOneWidget);
    final previousCalls = client.boardCalls.length;
    await tester.tap(find.text('새로고침'));
    await tester.pumpAndSettle();
    expect(client.boardCalls.length, greaterThan(previousCalls));
  });

  testWidgets('폰에서는 상태가 세로 구획으로 쌓인다', (tester) async {
    await tester.binding.setSurfaceSize(phoneSize);
    final client = FakeDesktopClient(board: boardWith());

    await tester.pumpWidget(
      wrapWithApp(const BoardScreen(), client: client, size: phoneSize),
    );
    await tester.pumpAndSettle();

    expect(find.byType(CustomScrollView), findsOneWidget);
    for (final label in ['Todo', 'Progress', 'Pending', 'Review', 'Done']) {
      expect(find.text(label), findsOneWidget, reason: '$label 구획 머리글이 보여야 한다');
    }
  });

  testWidgets('태블릿에서는 상태가 옆으로 늘어선 칸이 된다', (tester) async {
    await tester.binding.setSurfaceSize(tabletSize);
    final client = FakeDesktopClient(board: boardWith());

    await tester.pumpWidget(
      wrapWithApp(const BoardScreen(), client: client, size: tabletSize),
    );
    await tester.pumpAndSettle();

    expect(
      find.byType(CustomScrollView),
      findsNothing,
      reason: '칸반은 구획 목록을 쓰지 않는다',
    );
    final board = tester.widget<ListView>(find.byType(ListView).first);
    expect(board.scrollDirection, Axis.horizontal, reason: '칸반은 옆으로 밀어 본다');
  });

  testWidgets('태스크 제목과 프로젝트 이름이 함께 보인다', (tester) async {
    await tester.binding.setSurfaceSize(phoneSize);
    final client = FakeDesktopClient(board: boardWith());

    await tester.pumpWidget(
      wrapWithApp(const BoardScreen(), client: client, size: phoneSize),
    );
    await tester.pumpAndSettle();

    expect(find.text('할 일 하나'), findsOneWidget);
    expect(find.text('kanvibe'), findsOneWidget);
  });

  testWidgets('비어 있는 상태는 빈 자리 대신 안내를 보여 준다', (tester) async {
    await tester.binding.setSurfaceSize(phoneSize);
    final client = FakeDesktopClient(board: boardWith());

    await tester.pumpWidget(
      wrapWithApp(const BoardScreen(), client: client, size: phoneSize),
    );
    await tester.pumpAndSettle();

    expect(find.text('이 상태에는 태스크가 없습니다.'), findsWidgets);
  });

  testWidgets('보드의 현재 PC를 누르면 전환 목록이 열린다', (tester) async {
    await tester.binding.setSurfaceSize(phoneSize);
    await tester.pumpWidget(
      wrapWithApp(
        const BoardScreen(),
        client: FakeDesktopClient(board: boardWith()),
        size: phoneSize,
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.text('127.0.0.1'));
    await tester.pumpAndSettle();

    expect(find.byType(DesktopConnectionsScreen), findsOneWidget);
    expect(find.text('PC 추가'), findsOneWidget);
  });

  testWidgets('데스크탑에 닿지 못하면 되돌아갈 길을 준다', (tester) async {
    await tester.binding.setSurfaceSize(phoneSize);
    final client = _UnreachableDesktopClient();

    await tester.pumpWidget(
      wrapWithApp(const BoardScreen(), client: client, size: phoneSize),
    );
    await tester.pumpAndSettle();

    expect(find.textContaining('데스크탑에 연결하지 못했습니다'), findsOneWidget);
    expect(find.text('다시 시도'), findsOneWidget);
  });

  testWidgets('보드에서 새 태스크를 만들러 갈 수 있다', (tester) async {
    await tester.binding.setSurfaceSize(phoneSize);
    final client = FakeDesktopClient(board: boardWith());

    await tester.pumpWidget(
      wrapWithApp(const BoardScreen(), client: client, size: phoneSize),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byTooltip('새 태스크'));
    await tester.pumpAndSettle();

    expect(find.text('브랜치 이름 *'), findsOneWidget);
  });

  testWidgets('보드를 못 불러온 동안에는 만들기 버튼을 내보이지 않는다', (tester) async {
    await tester.binding.setSurfaceSize(phoneSize);

    await tester.pumpWidget(
      wrapWithApp(
        const BoardScreen(),
        client: _UnreachableDesktopClient(),
        size: phoneSize,
      ),
    );
    await tester.pumpAndSettle();

    expect(
      find.byType(FloatingActionButton),
      findsNothing,
      reason: '고를 프로젝트 목록도 함께 못 받은 상태다',
    );
  });
  group('길게 눌러 삭제', () {
    Future<FakeDesktopClient> pumpBoard(
      WidgetTester tester, {
      List<Object?> deleteErrors = const [],
      int failedPeerCount = 0,
    }) async {
      await tester.binding.setSurfaceSize(phoneSize);
      final client = FakeDesktopClient(
        board: boardWith(),
        deleteErrors: deleteErrors,
        failedPeerCount: failedPeerCount,
      );
      await tester.pumpWidget(
        wrapWithApp(const BoardScreen(), client: client, size: phoneSize),
      );
      await tester.pumpAndSettle();
      await tester.longPress(find.text('할 일 하나'));
      await tester.pumpAndSettle();
      return client;
    }

    testWidgets('길게 누르면 삭제를 묻고, 취소하면 아무것도 지우지 않는다', (tester) async {
      final client = await pumpBoard(tester);

      expect(find.text('이 작업을 삭제하시겠습니까?'), findsOneWidget);
      await tester.tap(find.text('취소'));
      await tester.pumpAndSettle();

      expect(client.deleteCalls, isEmpty);
    });

    testWidgets('확인하면 변경을 지키는 삭제를 보내고 보드를 다시 불러온다', (tester) async {
      final client = await pumpBoard(tester);
      final boardCallsBefore = client.boardCalls.length;

      await tester.tap(find.widgetWithText(TextButton, '삭제'));
      await tester.pumpAndSettle();

      expect(client.deleteCalls, [(taskId: 't1', isForced: false)]);
      expect(client.boardCalls.length, greaterThan(boardCallsBefore));
      expect(find.byType(SnackBar), findsNothing);
    });

    testWidgets('꺼져 있던 다른 데스크탑에는 남았다고 알린다', (tester) async {
      await pumpBoard(tester, failedPeerCount: 2);

      await tester.tap(find.widgetWithText(TextButton, '삭제'));
      await tester.pumpAndSettle();

      expect(
        find.text('다른 데스크탑 2곳에는 아직 남아 있습니다. 그 데스크탑이 켜지면 직접 삭제해 주세요.'),
        findsOneWidget,
      );
    });

    testWidgets('삭제 응답 전에 새로고침으로 카드가 사라져도 결과를 알린다', (tester) async {
      await tester.binding.setSurfaceSize(phoneSize);
      final board = boardWith();
      final pending = Completer<Map<String, dynamic>>();
      final client = _DelayedDeletionClient(board: board, pending: pending);
      await tester.pumpWidget(
        wrapWithApp(const BoardScreen(), client: client, size: phoneSize),
      );
      await tester.pumpAndSettle();
      await tester.longPress(find.text('할 일 하나'));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(TextButton, '삭제'));
      // 삭제 중 표시가 계속 움직이므로 settle 대신 화면 전환 시간만큼 넘긴다
      await tester.pump(const Duration(seconds: 1));

      (board['tasks'] as Map<String, dynamic>)['todo'] =
          <Map<String, dynamic>>[];
      await tester.tap(find.byTooltip('보드 메뉴'));
      await tester.pump();
      await tester.pump(const Duration(seconds: 1));
      await tester.tap(find.text('새로고침'));
      await tester.pumpAndSettle();
      expect(find.text('할 일 하나'), findsNothing);

      pending.complete({'ok': true, 'failedPeerCount': 2});
      await tester.pumpAndSettle();

      expect(tester.takeException(), isNull);
      expect(
        find.text('다른 데스크탑 2곳에는 아직 남아 있습니다. 그 데스크탑이 켜지면 직접 삭제해 주세요.'),
        findsOneWidget,
      );
    });

    testWidgets('응답을 기다리는 카드는 삭제 중으로 보이고 다시 길게 눌러도 또 지우지 않는다', (tester) async {
      await tester.binding.setSurfaceSize(phoneSize);
      final pending = Completer<Map<String, dynamic>>();
      final client = _DelayedDeletionClient(
        board: boardWith(),
        pending: pending,
      );
      await tester.pumpWidget(
        wrapWithApp(const BoardScreen(), client: client, size: phoneSize),
      );
      await tester.pumpAndSettle();
      await tester.longPress(find.text('할 일 하나'));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(TextButton, '삭제'));
      await tester.pump();

      final card = find.byKey(const ValueKey('t1'));
      expect(
        find.descendant(of: card, matching: find.text('삭제하는 중…')),
        findsOneWidget,
      );
      expect(
        find.descendant(
          of: card,
          matching: find.byType(LinearProgressIndicator),
        ),
        findsOneWidget,
      );

      await tester.longPress(find.text('할 일 하나'));
      await tester.pump();
      expect(find.text('이 작업을 삭제하시겠습니까?'), findsNothing);
      expect(client.deleteRequestCount, 1);

      pending.complete({'ok': true, 'failedPeerCount': 0});
      await tester.pumpAndSettle();
      expect(find.text('삭제하는 중…'), findsNothing);
    });

    testWidgets('커밋하지 않은 변경이 있으면 강제 삭제를 다시 묻고 확인하면 강제로 보낸다', (tester) async {
      final client = await pumpBoard(
        tester,
        deleteErrors: const [
          DesktopCommandFailure('dirty-worktree', '커밋하지 않은 변경이 있습니다'),
        ],
      );

      await tester.tap(find.widgetWithText(TextButton, '삭제'));
      await tester.pumpAndSettle();
      expect(find.text('커밋하지 않은 변경이 있습니다'), findsOneWidget);
      await tester.tap(find.text('강제 삭제'));
      await tester.pumpAndSettle();

      expect(client.deleteCalls, [
        (taskId: 't1', isForced: false),
        (taskId: 't1', isForced: true),
      ]);
    });

    testWidgets('강제 삭제를 취소하면 더 지우지 않는다', (tester) async {
      final client = await pumpBoard(
        tester,
        deleteErrors: const [
          DesktopCommandFailure('dirty-worktree', '커밋하지 않은 변경이 있습니다'),
        ],
      );

      await tester.tap(find.widgetWithText(TextButton, '삭제'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('취소'));
      await tester.pumpAndSettle();

      expect(client.deleteCalls, [(taskId: 't1', isForced: false)]);
    });

    testWidgets('실패하면 사유를 SnackBar로 알리고 카드를 그대로 둔다', (tester) async {
      await pumpBoard(
        tester,
        deleteErrors: const [DesktopCommandFailure('server-error', '서버 오류')],
      );

      await tester.tap(find.widgetWithText(TextButton, '삭제'));
      await tester.pumpAndSettle();

      expect(find.text('서버 오류'), findsOneWidget);
      expect(find.text('할 일 하나'), findsOneWidget);
    });

    testWidgets('깨진 저장소는 확인한 뒤 등록만 제거하고 보드를 갱신한다', (tester) async {
      final client = await pumpBoard(
        tester,
        deleteErrors: const [
          DesktopCommandFailure('invalid-worktree', 'Git 저장소를 찾을 수 없습니다'),
        ],
      );
      await tester.tap(find.widgetWithText(TextButton, '삭제'));
      await tester.pumpAndSettle();
      expect(find.text('태스크 등록만 제거할까요?'), findsOneWidget);
      expect(client.removedRegistrationIds, isEmpty);
      final previousFetches = client.boardCalls.length;
      await tester.tap(find.widgetWithText(TextButton, '등록만 제거'));
      await tester.pumpAndSettle();
      expect(client.removedRegistrationIds, ['t1']);
      expect(client.deleteCalls, [(taskId: 't1', isForced: false)]);
      expect(client.boardCalls.length, greaterThan(previousFetches));
    });

    testWidgets('등록 제거를 취소하면 서버에 추가 명령을 보내지 않는다', (tester) async {
      final client = await pumpBoard(
        tester,
        deleteErrors: const [
          DesktopCommandFailure('invalid-worktree', 'Git 저장소를 찾을 수 없습니다'),
        ],
      );
      await tester.tap(find.widgetWithText(TextButton, '삭제'));
      await tester.pumpAndSettle();
      expect(find.text('태스크 등록만 제거할까요?'), findsOneWidget);
      await tester.tap(find.text('취소'));
      await tester.pumpAndSettle();
      expect(client.removedRegistrationIds, isEmpty);
      expect(find.text('할 일 하나'), findsOneWidget);
    });

    testWidgets('데스크탑이 명령을 모르면 업데이트 안내를 띄운다', (tester) async {
      await pumpBoard(
        tester,
        deleteErrors: const [DesktopUpdateRequiredException()],
      );

      await tester.tap(find.widgetWithText(TextButton, '삭제'));
      await tester.pumpAndSettle();

      expect(find.text('데스크탑 앱을 업데이트해 주세요'), findsOneWidget);
    });

    testWidgets('연결이 끊겨 있으면 끊김 안내를 띄운다', (tester) async {
      await pumpBoard(
        tester,
        deleteErrors: const [DesktopDisconnectedException()],
      );

      await tester.tap(find.widgetWithText(TextButton, '삭제'));
      await tester.pumpAndSettle();

      expect(find.text('데스크탑과 연결이 끊겼습니다. 연결을 확인해 주세요.'), findsOneWidget);
    });
  });
}

class _DelayedDeletionClient extends FakeDesktopClient {
  _DelayedDeletionClient({required super.board, required this.pending});

  final Completer<Map<String, dynamic>> pending;
  int deleteRequestCount = 0;

  @override
  Future<Map<String, dynamic>> runCommand(
    connection,
    DesktopCommand command, {
    Duration? timeout,
  }) {
    deleteRequestCount++;
    return pending.future;
  }
}

class _UnreachableDesktopClient extends FakeDesktopClient {
  @override
  Future<Map<String, dynamic>> fetchBoard(connection) async =>
      throw const DesktopUnreachable();
}

class DesktopUnreachable implements Exception {
  const DesktopUnreachable();
}
