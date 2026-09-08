import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/features/board/presentation/board_screen.dart';

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
}

class _UnreachableDesktopClient extends FakeDesktopClient {
  @override
  Future<Map<String, dynamic>> fetchBoard(connection) async =>
      throw const DesktopUnreachable();
}

class DesktopUnreachable implements Exception {
  const DesktopUnreachable();
}
