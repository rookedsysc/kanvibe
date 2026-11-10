import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_command.dart';
import 'package:kanvibe_mobile/src/features/board/presentation/board_screen.dart';
import 'package:kanvibe_mobile/src/features/connection/presentation/desktop_connections_screen.dart';

import '../../../support/fake_desktop.dart';

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
      await tester.pumpAndSettle();

      (board['tasks'] as Map<String, dynamic>)['todo'] =
          <Map<String, dynamic>>[];
      await tester.tap(find.byTooltip('새로고침'));
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

  @override
  Future<Map<String, dynamic>> runCommand(
    connection,
    DesktopCommand command, {
    Duration? timeout,
  }) => pending.future;
}

class _UnreachableDesktopClient extends FakeDesktopClient {
  @override
  Future<Map<String, dynamic>> fetchBoard(connection) async =>
      throw const DesktopUnreachable();
}

class DesktopUnreachable implements Exception {
  const DesktopUnreachable();
}
