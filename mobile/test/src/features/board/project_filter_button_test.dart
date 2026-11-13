import 'dart:async';

import 'package:flutter/material.dart';
import 'package:kanvibe_mobile/src/features/board/data/board_snapshot.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/constants/app_theme.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/board/presentation/board_screen.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/task_create/presentation/create_task_screen.dart';

import '../../../support/fake_desktop.dart';

const _smallPhone = Size(375, 812);

Map<String, dynamic> _filterBoard({String name = 'kanvibe'}) => {
  'defaultSessionType': 'terminal',
  'projects': [
    {
      'id': 'local',
      'name': name,
      'repoPath': '/code/kanvibe',
      'color': '#93C5FD',
    },
    {
      'id': 'worktree',
      'name': 'kanvibe-worktree',
      'repoPath': '/code/kanvibe__worktrees/feature',
      'isWorktree': true,
    },
    {
      'id': 'remote',
      'name': 'kanvibe',
      'repoPath': '/code/kanvibe',
      'sshHost': 'devbox',
    },
    {
      'id': 'remote-worktree',
      'name': 'remote-worktree',
      'repoPath': '/code/kanvibe__worktrees/remote',
      'sshHost': 'devbox',
      'isWorktree': true,
    },
    {'id': 'other', 'name': 'docs', 'repoPath': '/code/docs'},
  ],
  'tasks': {
    'todo': [
      for (final id in [
        'local',
        'worktree',
        'remote',
        'remote-worktree',
        'other',
      ])
        {
          'id': 'task-$id',
          'title': '$id 태스크',
          'status': 'todo',
          'projectId': id,
        },
    ],
  },
};

Widget _app({
  required FakeDesktopClient client,
  Size size = _smallPhone,
  double textScale = 1,
  FakeConnectionStore? store,
}) => ProviderScope(
  overrides: [
    desktopClientProvider.overrideWithValue(client),
    connectionStoreProvider.overrideWithValue(store ?? FakeConnectionStore()),
  ],
  child: MaterialApp(
    theme: buildAppTheme(),
    builder: (context, child) => MediaQuery(
      data: MediaQuery.of(context).copyWith(
        size: size,
        textScaler: TextScaler.linear(textScale),
        disableAnimations: true,
      ),
      child: child!,
    ),
    home: const BoardScreen(),
  ),
);

Finder _control(String id) => find.byKey(ValueKey(id));

Future<void> _openFilter(WidgetTester tester) async {
  await tester.tap(_control('project-filter'));
  await tester.pumpAndSettle();
}

Future<void> _closeFilter(WidgetTester tester) async {
  await tester.tap(_control('project-filter-close'));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('데스크톱을 연속 복수 선택하고 프로젝트 필터와 함께 적용한다', (tester) async {
    await tester.pumpWidget(
      _app(client: FakeDesktopClient(board: _filterBoard())),
    );
    await tester.pumpAndSettle();
    final trigger = find.descendant(
      of: _control('desktop-filter'),
      matching: find.byType(IconButton),
    );
    final unselectedColor = tester.widget<IconButton>(trigger).color;
    final desktopBadge = find.descendant(
      of: _control('desktop-filter'),
      matching: find.byType(Badge),
    );
    expect(tester.widget<Badge>(desktopBadge).isLabelVisible, isFalse);
    await tester.tap(_control('desktop-filter'));
    await tester.pumpAndSettle();
    await tester.tap(_control('desktop-filter-ssh-devbox'));
    await tester.pumpAndSettle();
    await tester.tap(_control('desktop-filter-close'));
    await tester.pumpAndSettle();
    expect(find.text('local 태스크'), findsNothing);
    expect(find.text('remote 태스크'), findsOneWidget);
    expect(tester.widget<IconButton>(trigger).color, unselectedColor);
    expect(
      find.descendant(of: desktopBadge, matching: find.text('1')),
      findsOneWidget,
    );
    await tester.tap(_control('desktop-filter'));
    await tester.pumpAndSettle();
    await tester.tap(_control('desktop-filter-local'));
    await tester.pumpAndSettle();
    await tester.tap(_control('desktop-filter-close'));
    await tester.pumpAndSettle();
    expect(find.text('local 태스크'), findsOneWidget);
    expect(find.text('remote 태스크'), findsOneWidget);
    final selectedCount = find.descendant(
      of: desktopBadge,
      matching: find.text('2'),
    );
    expect(selectedCount, findsOneWidget);
    await tester.tap(selectedCount);
    await tester.pumpAndSettle();
    await tester.tap(_control('desktop-filter-local'));
    await tester.pumpAndSettle();
    await tester.tap(_control('desktop-filter-close'));
    await tester.pumpAndSettle();
    await _openFilter(tester);
    await tester.tap(_control('project-filter-option-local'));
    await _closeFilter(tester);
    expect(find.text('remote 태스크'), findsNothing);
    await tester.tap(_control('desktop-filter'));
    await tester.pumpAndSettle();
    await tester.tap(_control('desktop-filter-all'));
    await tester.pumpAndSettle();
    await tester.tap(_control('desktop-filter-close'));
    await tester.pumpAndSettle();
    expect(find.text('local 태스크'), findsOneWidget);
    expect(find.text('remote 태스크'), findsNothing);
    expect(tester.widget<Badge>(desktopBadge).isLabelVisible, isFalse);
    expect(tester.takeException(), isNull);
  });

  testWidgets('연속 입력은 마지막 fuzzy 검색만 적용하고 보드를 추가 요청하지 않는다', (tester) async {
    final client = FakeDesktopClient(board: _filterBoard());
    await tester.pumpWidget(_app(client: client));
    await tester.pumpAndSettle();
    await _openFilter(tester);
    await tester.enterText(find.byType(TextField), 'dv');
    await tester.pump(const Duration(milliseconds: 200));
    await tester.enterText(find.byType(TextField), 'DVBX');
    await tester.pump(const Duration(milliseconds: 299));
    expect(_control('project-filter-option-local'), findsOneWidget);
    await tester.pump(const Duration(milliseconds: 1));
    expect(_control('project-filter-option-remote'), findsOneWidget);
    expect(_control('project-filter-option-local'), findsNothing);
    expect(client.boardCalls, hasLength(1));
    await tester.enterText(find.byType(TextField), 'kvb');
    await _closeFilter(tester);
    await tester.pump(const Duration(milliseconds: 300));
    expect(tester.takeException(), isNull);
  });

  testWidgets('보드 갱신 중에도 검색 결과와 입력을 유지한다', (tester) async {
    final pending = Completer<BoardSnapshot>();
    var refreshing = false;
    final container = ProviderContainer(
      overrides: [
        desktopClientProvider.overrideWithValue(FakeDesktopClient()),
        boardProvider.overrideWith(
          (ref) => refreshing
              ? pending.future
              : Future.value(BoardSnapshot.fromJson(_filterBoard())),
        ),
        connectionStoreProvider.overrideWithValue(FakeConnectionStore()),
      ],
    );
    addTearDown(container.dispose);
    await tester.pumpWidget(
      UncontrolledProviderScope(
        container: container,
        child: MaterialApp(theme: buildAppTheme(), home: const BoardScreen()),
      ),
    );
    await tester.pumpAndSettle();
    await _openFilter(tester);
    await tester.enterText(find.byType(TextField), 'kanvibe');
    await tester.pump(const Duration(milliseconds: 300));
    refreshing = true;
    container.invalidate(boardProvider);
    await tester.pump();
    expect(_control('project-filter-option-local'), findsOneWidget);
    expect(find.byType(CircularProgressIndicator), findsNothing);
    expect(
      tester.widget<EditableText>(find.byType(EditableText)).controller.text,
      'kanvibe',
    );
    pending.complete(BoardSnapshot.fromJson(_filterBoard()));
    await tester.pumpAndSettle();
  });

  testWidgets('본 프로젝트와 같은 호스트의 worktree를 복수 선택하고 전체로 복원한다', (tester) async {
    await tester.binding.setSurfaceSize(_smallPhone);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await tester.pumpWidget(
      _app(client: FakeDesktopClient(board: _filterBoard())),
    );
    await tester.pumpAndSettle();

    await _openFilter(tester);
    expect(_control('project-filter-option-worktree'), findsNothing);
    expect(_control('project-filter-option-remote-worktree'), findsNothing);
    await tester.tap(_control('project-filter-option-local'));
    await _closeFilter(tester);

    expect(find.text('local 태스크'), findsOneWidget);
    expect(find.text('worktree 태스크'), findsOneWidget);
    expect(find.text('remote 태스크'), findsNothing);
    expect(find.text('remote-worktree 태스크'), findsNothing);
    expect(find.text('other 태스크'), findsNothing);

    await _openFilter(tester);
    await tester.tap(_control('project-filter-option-remote'));
    await _closeFilter(tester);
    expect(find.text('remote 태스크'), findsOneWidget);
    expect(find.text('remote-worktree 태스크'), findsOneWidget);
    expect(find.text('other 태스크'), findsNothing);

    await _openFilter(tester);
    await tester.tap(_control('project-filter-all'));
    await _closeFilter(tester);
    expect(find.text('other 태스크'), findsOneWidget);
  });

  testWidgets('프로젝트 이름·경로·호스트를 검색하고 다시 열 때 검색만 초기화한다', (tester) async {
    await tester.binding.setSurfaceSize(_smallPhone);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await tester.pumpWidget(
      _app(client: FakeDesktopClient(board: _filterBoard())),
    );
    await tester.pumpAndSettle();
    await _openFilter(tester);
    expect(find.text('로컬 · /code/kanvibe'), findsOneWidget);
    expect(find.text('devbox · /code/kanvibe'), findsOneWidget);

    await tester.enterText(find.byType(TextField), 'DEVBOX');
    await tester.pump(const Duration(milliseconds: 300));
    await tester.pumpAndSettle();
    expect(_control('project-filter-option-remote'), findsOneWidget);
    expect(_control('project-filter-option-local'), findsNothing);
    await tester.tap(_control('project-filter-option-remote'));

    await tester.enterText(find.byType(TextField), '/code/docs');
    await tester.pump(const Duration(milliseconds: 300));
    await tester.pumpAndSettle();
    expect(_control('project-filter-option-other'), findsOneWidget);
    await tester.enterText(find.byType(TextField), '없는 프로젝트');
    await tester.pump(const Duration(milliseconds: 300));
    await tester.pumpAndSettle();
    expect(find.text('검색 결과가 없습니다.'), findsOneWidget);
    await _closeFilter(tester);
    await _openFilter(tester);
    expect(
      tester.widget<EditableText>(find.byType(EditableText)).controller.text,
      '',
    );
    expect(_control('project-filter-option-local'), findsOneWidget);
    final selected = tester.widget<CheckboxListTile>(
      find.descendant(
        of: _control('project-filter-option-remote'),
        matching: find.byType(CheckboxListTile),
      ),
    );
    expect(selected.value, isTrue);
  });

  testWidgets('필터를 적용해도 생성 화면은 전체 본 프로젝트와 기본 세션 종류를 받는다', (tester) async {
    await tester.binding.setSurfaceSize(_smallPhone);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await tester.pumpWidget(
      _app(client: FakeDesktopClient(board: _filterBoard())),
    );
    await tester.pumpAndSettle();
    await _openFilter(tester);
    await tester.tap(_control('project-filter-option-local'));
    await _closeFilter(tester);
    await tester.tap(find.byTooltip('새 태스크'));
    await tester.pumpAndSettle();

    final createScreen = tester.widget<CreateTaskScreen>(
      find.byType(CreateTaskScreen),
    );
    expect(createScreen.projects.map((project) => project.id), [
      'local',
      'remote',
      'other',
    ]);
  });

  testWidgets('PC를 바꾸면 선택이 섞이지 않고 되돌아오면 이전 선택을 복원한다', (tester) async {
    const first = DesktopConnection(
      host: 'first',
      port: 9736,
      token: 'first-token',
    );
    const second = DesktopConnection(
      host: 'second',
      port: 9736,
      token: 'second-token',
    );
    await tester.binding.setSurfaceSize(_smallPhone);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await tester.pumpWidget(
      _app(
        client: FakeDesktopClient(board: _filterBoard()),
        store: FakeConnectionStore(
          initial: const SavedConnections(
            connections: [first, second],
            activeId: 'first:9736',
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    final container = ProviderScope.containerOf(
      tester.element(find.byType(BoardScreen)),
    );
    await _openFilter(tester);
    await tester.tap(_control('project-filter-option-local'));
    await _closeFilter(tester);

    await container
        .read(connectionControllerProvider.notifier)
        .select(second.id);
    await tester.pumpAndSettle();
    expect(find.text('other 태스크'), findsOneWidget);
    await _openFilter(tester);
    await tester.tap(_control('project-filter-option-other'));
    await _closeFilter(tester);
    expect(find.text('local 태스크'), findsNothing);
    expect(find.text('other 태스크'), findsOneWidget);

    await container
        .read(connectionControllerProvider.notifier)
        .select(first.id);
    await tester.pumpAndSettle();
    expect(find.text('local 태스크'), findsOneWidget);
    expect(find.text('other 태스크'), findsNothing);
  });

  for (final size in [_smallPhone, const Size(812, 375), tabletSize]) {
    testWidgets('긴 프로젝트 이름과 2배 글자를 ${size.width}×${size.height}에서 표시한다', (
      tester,
    ) async {
      await tester.binding.setSurfaceSize(size);
      addTearDown(() => tester.binding.setSurfaceSize(null));
      await tester.pumpWidget(
        _app(
          client: FakeDesktopClient(
            board: _filterBoard(name: '긴 프로젝트 이름 ' * 12),
          ),
          size: size,
          textScale: 2,
        ),
      );
      await tester.pumpAndSettle();
      await _openFilter(tester);
      expect(tester.takeException(), isNull);
      expect(
        tester.getSize(_control('project-filter-all')).height,
        greaterThanOrEqualTo(48),
      );
      await tester.ensureVisible(_control('project-filter-option-local'));
      await tester.pumpAndSettle();
      expect(
        tester.getSize(_control('project-filter-option-local')).height,
        greaterThanOrEqualTo(48),
      );
      await tester.tap(_control('project-filter-option-local'));
      await _closeFilter(tester);
      expect(find.text('local 태스크'), findsOneWidget);
      expect(tester.takeException(), isNull);
    });
  }
}
