import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/constants/app_theme.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_command.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/board/data/board_snapshot.dart';
import 'package:kanvibe_mobile/src/features/board/domain/board_task.dart';
import 'package:kanvibe_mobile/src/features/board/domain/task_status.dart';
import 'package:kanvibe_mobile/src/features/task_create/domain/session_type.dart';
import 'package:kanvibe_mobile/src/features/task_create/presentation/create_task_screen.dart';
import 'package:kanvibe_mobile/src/features/task_detail/domain/mirror_pane.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/task_detail_screen.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/pane_terminal_view.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/terminal_chrome_header.dart';
import 'package:xterm/xterm.dart' as xterm;

import '../../../support/fake_desktop.dart';

/// 데스크탑 스크린샷과 같은 모양. window 하나가 좌우 두 pane으로 갈려 있고, 두 번째 window가 하나 더 있다
const _twoPaneWindow = MirrorTab(
  id: '@10',
  name: 'kanvibe',
  panes: [
    MirrorPane(
      id: '%19',
      tabId: '@10',
      tabName: 'kanvibe',
      command: 'claude',
      left: 0,
      top: 0,
      width: 50,
      height: 30,
    ),
    MirrorPane(
      id: '%20',
      tabId: '@10',
      tabName: 'kanvibe',
      command: 'zsh',
      left: 51,
      top: 0,
      width: 49,
      height: 30,
    ),
  ],
);

const _secondWindow = MirrorTab(
  id: '@11',
  name: 'second',
  panes: [
    MirrorPane(
      id: '%21',
      tabId: '@11',
      tabName: 'second',
      command: 'bash',
      left: 0,
      top: 0,
      width: 100,
      height: 30,
    ),
  ],
);

const _surfaces = TaskSurfaces(
  tabs: [_twoPaneWindow, _secondWindow],
  canResize: true,
);

const _createdTab = MirrorTab(
  id: '@12',
  name: 'new-shell',
  panes: [
    MirrorPane(
      id: '%22',
      tabId: '@12',
      tabName: 'new-shell',
      command: 'zsh',
      left: 0,
      top: 0,
      width: 80,
      height: 24,
    ),
  ],
);
const _createdSurfaces = TaskSurfaces(
  tabs: [_twoPaneWindow, _secondWindow, _createdTab],
  canResize: true,
);

const _task = BoardTask(
  id: 'task-1',
  title: '모바일 클라이언트',
  status: TaskStatus.progress,
  branchName: 'feat/kanvibe-mobile',
  baseBranch: 'dev',

  /// tmux window 이름과 겹치지 않는 이름을 쓴다. 겹치면 머리글의 프로젝트와 탭 이름을 가려낼 수 없다
  projectName: 'kanvibe-repo',
);

Future<FakeDesktopClient> pumpDetail(
  WidgetTester tester, {
  required Size size,
  TaskSurfaces? surfaces = _surfaces,
  Object? surfacesError,
  BoardTask task = _task,
  Object? statusError,
  FakeDesktopClient? desktopClient,
}) async {
  await tester.binding.setSurfaceSize(size);
  final client =
      desktopClient ??
      FakeDesktopClient(
        surfaces: surfaces,
        surfacesError: surfacesError,
        statusError: statusError,
      );

  await tester.pumpWidget(
    wrapWithApp(
      TaskDetailScreen(task: task),
      client: client,
      size: size,
    ),
  );
  await tester.pumpAndSettle();
  return client;
}

/// pane이 실제로 키를 받는 자리. 입력이 어느 소켓으로 나가는지는 여기서만 확인할 수 있다
xterm.Terminal terminalIn(WidgetTester tester, String paneId) => tester
    .widget<xterm.TerminalView>(
      find.descendant(
        of: find.byKey(ValueKey(paneId)),
        matching: find.byType(xterm.TerminalView),
      ),
    )
    .terminal;

const _creationBoardJson = {
  'projects': [
    {'id': 'project-1', 'name': 'kanvibe-repo', 'defaultBranch': 'dev'},
  ],
  'tasks': {},
  'defaultSessionType': 'zellij',
};

const _creationSource = BoardTask(
  id: 'source-task',
  title: '원본 태스크',
  status: TaskStatus.progress,
  projectId: 'project-1',
  projectName: 'kanvibe-repo',
  branchName: 'feat/source',
  baseBranch: 'dev',
);

Future<FakeDesktopClient> _pumpCreationDetail(
  WidgetTester tester, {
  BoardTask task = _creationSource,
  Map<String, dynamic> board = _creationBoardJson,
  Future<BoardSnapshot> Function()? loadBoard,
  Object? createError,
  Size size = phoneSize,
}) async {
  await tester.binding.setSurfaceSize(size);
  final client = FakeDesktopClient(
    board: board,
    surfaces: _surfaces,
    createError: createError,
    createdTask: const {
      'id': 'created-task',
      'title': 'feat/child',
      'status': 'todo',
      'projectId': 'project-1',
      'branchName': 'feat/child',
    },
  );
  final app = ProviderScope(
    overrides: [
      desktopClientProvider.overrideWithValue(client),
      connectionStoreProvider.overrideWithValue(FakeConnectionStore()),
      if (loadBoard != null) boardProvider.overrideWith((_) => loadBoard()),
    ],
    child: MaterialApp(
      theme: buildAppTheme(),
      home: Builder(
        builder: (context) => TextButton(
          onPressed: () => Navigator.of(context).push(
            MaterialPageRoute<void>(
              builder: (_) => TaskDetailScreen(task: task),
            ),
          ),
          child: const Text('보드 화면'),
        ),
      ),
    ),
  );
  await tester.pumpWidget(app);
  await tester.pumpAndSettle();
  await tester.tap(find.text('보드 화면'));
  await tester.pumpAndSettle();
  return client;
}

Future<void> _openCreation(WidgetTester tester) async {
  await tester.tap(find.bySemanticsIdentifier('create-task-from-current'));
  await tester.pumpAndSettle();
}

Future<void> _submitCreation(WidgetTester tester) async {
  await tester.ensureVisible(find.widgetWithText(FilledButton, '만들기'));
  await tester.tap(find.widgetWithText(FilledButton, '만들기'));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('탭 추가는 새 pane을 선택하고 새 셸로 입력을 보낸다', (tester) async {
    final client = FakeDesktopClient(
      surfaces: _surfaces,
      onCreateTerminalTab: () async =>
          (surfaces: _createdSurfaces, paneId: '%22'),
    );
    await pumpDetail(tester, size: phoneSize, desktopClient: client);
    final add = find.bySemanticsIdentifier('create-terminal-tab');
    expect(tester.getSize(add), const Size(48, 48));
    await tester.tap(add);
    await tester.pumpAndSettle();
    expect(client.createdTabCalls, [
      (taskId: 'task-1', connectionId: '127.0.0.1:9736'),
    ]);
    expect(
      tester.widget<PaneTerminalView>(find.byType(PaneTerminalView)).pane.id,
      '%22',
    );
    terminalIn(tester, '%22').textInput('pwd');
    expect(client.writtenInput['%22'], ['pwd']);
    expect(client.writtenInput['%19'], isNull);
    expect(client.closedPanes, contains('%19'));
    expect(find.bySemanticsIdentifier('create-terminal-tab'), findsOneWidget);
  });

  testWidgets('추가 중 반복 탭은 한 요청이며 실패 후 기존 pane에서 다시 추가할 수 있다', (tester) async {
    final pending = Completer<({TaskSurfaces surfaces, String paneId})>();
    var attempts = 0;
    final client = FakeDesktopClient(
      surfaces: _surfaces,
      onCreateTerminalTab: () {
        attempts += 1;
        return attempts == 1
            ? pending.future
            : Future.value((surfaces: _createdSurfaces, paneId: '%22'));
      },
    );
    await pumpDetail(tester, size: phoneSize, desktopClient: client);
    final terminal = terminalIn(tester, '%19');
    final add = find.bySemanticsIdentifier('create-terminal-tab');
    await tester.tap(add);
    await tester.pump();
    await tester.tap(add);
    await tester.pump();
    expect(client.createdTabCalls, hasLength(1));
    expect(terminalIn(tester, '%19'), same(terminal));
    pending.completeError(const DesktopRequestException(500));
    await tester.pumpAndSettle();
    expect(find.textContaining('터미널 탭을 추가하지 못했습니다'), findsOneWidget);
    expect(terminalIn(tester, '%19'), same(terminal));
    await tester.tap(add);
    await tester.pumpAndSettle();
    expect(client.createdTabCalls, hasLength(2));
    expect(
      tester.widget<PaneTerminalView>(find.byType(PaneTerminalView)).pane.id,
      '%22',
    );
  });

  testWidgets('탭 추가 중 화면을 떠나면 완료 응답이 화면을 다시 열지 않는다', (tester) async {
    final pending = Completer<({TaskSurfaces surfaces, String paneId})>();
    final client = FakeDesktopClient(
      surfaces: _surfaces,
      onCreateTerminalTab: () => pending.future,
    );
    await pumpDetail(tester, size: phoneSize, desktopClient: client);
    await tester.tap(find.bySemanticsIdentifier('create-terminal-tab'));
    await tester.pump();
    await tester.pumpWidget(const SizedBox.shrink());
    pending.complete((surfaces: _createdSurfaces, paneId: '%22'));
    await tester.pumpAndSettle();
    expect(find.byType(TaskDetailScreen), findsNothing);
    expect(tester.takeException(), isNull);
  });

  for (final session in ['tmux', 'terminal', 'zellij', null]) {
    testWidgets(
      '${session ?? 'sessionType 누락'} 상세는 서버의 device query 응답 소유권을 전달한다',
      (tester) async {
        final surfaces = TaskSurfaces.fromJson({
          'sessionType': ?session,
          'tabs': [
            {
              'id': '@10',
              'panes': [
                {'id': '%19', 'width': 80, 'height': 24},
              ],
            },
          ],
        });
        final client = await pumpDetail(
          tester,
          size: phoneSize,
          surfaces: surfaces,
        );
        client.openedPanes['%19']!.add('\x1b[6n');
        await tester.pumpAndSettle();
        final responses = client.writtenInput['%19'] ?? [];
        expect(responses, session == 'terminal' ? ['\x1b[0;0R'] : isEmpty);
        terminalIn(tester, '%19').textInput('user');
        expect(client.writtenInput['%19']!.last, 'user');
      },
    );
  }

  testWidgets('현재 태스크에서 새 태스크를 열면 source 프로젝트와 브랜치가 선택된다', (tester) async {
    final client = FakeDesktopClient(
      board: const {
        'projects': [
          {'id': 'project-1', 'name': 'kanvibe-repo', 'defaultBranch': 'dev'},
        ],
        'tasks': {},
        'defaultSessionType': 'zellij',
      },
    );
    final source = BoardTask.fromJson(
      {
        'id': 'source-task',
        'title': '원본 태스크',
        'status': 'progress',
        'projectId': 'project-1',
        'branchName': 'feat/source',
        'baseBranch': 'dev',
        'description': '원본 설명',
        'priority': 'high',
        'sessionType': 'tmux',
      },
      const {'project-1': 'kanvibe-repo'},
    )!;
    await tester.binding.setSurfaceSize(phoneSize);
    await tester.pumpWidget(
      wrapWithApp(
        TaskDetailScreen(task: source),
        client: client,
        size: phoneSize,
      ),
    );
    await tester.pumpAndSettle();

    final createAction = find.bySemanticsIdentifier('create-task-from-current');
    expect(createAction, findsOneWidget);
    await tester.tap(createAction);
    await tester.pumpAndSettle();

    expect(find.byType(CreateTaskScreen), findsOneWidget);
    expect(find.text('kanvibe-repo'), findsOneWidget);
    expect(find.text('feat/source'), findsOneWidget);
    for (final field in tester.widgetList<TextFormField>(
      find.byType(TextFormField),
    )) {
      expect(field.controller!.text, isEmpty);
    }
    expect(find.text('고르지 않음'), findsOneWidget);

    await tester.enterText(find.byType(TextFormField).first, 'feat/child');
    await _submitCreation(tester);

    expect(client.createCalls.single, {
      'projectId': 'project-1',
      'branchName': 'feat/child',
      'baseBranch': 'feat/source',
      'description': '',
      'priority': null,
    });
  });

  group('현재 태스크에서 생성', () {
    for (final (branchName, baseBranch, expectedBase) in [
      (null, 'release/source', 'release/source'),
      (null, null, 'dev'),
    ]) {
      testWidgets(
        'source 브랜치 $branchName, base $baseBranch이면 $expectedBase에서 시작한다',
        (tester) async {
          await _pumpCreationDetail(
            tester,
            task: BoardTask(
              id: 'source-task',
              title: '원본 태스크',
              status: TaskStatus.progress,
              projectId: 'project-1',
              projectName: 'kanvibe-repo',
              branchName: branchName,
              baseBranch: baseBranch,
            ),
          );

          await _openCreation(tester);

          expect(find.byType(CreateTaskScreen), findsOneWidget);
          expect(find.text(expectedBase), findsOneWidget);
        },
      );
    }

    for (final projectId in [null, 'deleted-project']) {
      testWidgets('source 프로젝트 $projectId이면 현재 상세에서 원본 프로젝트 확인을 안내한다', (
        tester,
      ) async {
        final client = await _pumpCreationDetail(
          tester,
          task: BoardTask(
            id: 'source-task',
            title: '원본 태스크',
            status: TaskStatus.progress,
            projectId: projectId,
            branchName: 'feat/source',
          ),
        );

        await _openCreation(tester);

        expect(find.byType(CreateTaskScreen), findsNothing);
        expect(find.text('feat/source'), findsOneWidget);
        expect(find.textContaining('원본 프로젝트를 찾을 수 없습니다'), findsOneWidget);
        expect(client.createCalls, isEmpty);
      });
    }

    testWidgets('worktree 자체로 등록한 프로젝트에서는 다른 프로젝트를 추측하지 않는다', (tester) async {
      final client = await _pumpCreationDetail(
        tester,
        board: const {
          'projects': [
            {'id': 'project-1', 'name': 'kanvibe-repo', 'isWorktree': true},
          ],
          'tasks': {},
        },
      );

      await _openCreation(tester);

      expect(find.byType(CreateTaskScreen), findsNothing);
      expect(find.textContaining('원본 프로젝트를 찾을 수 없습니다'), findsOneWidget);
      expect(client.createCalls, isEmpty);
    });

    testWidgets('최신 보드를 못 받으면 상세를 유지하고 다시 누르면 재시도한다', (tester) async {
      var loads = 0;
      await _pumpCreationDetail(
        tester,
        loadBoard: () async {
          if (loads++ == 0) {
            throw const DesktopRequestException(500);
          }
          return BoardSnapshot.fromJson(_creationBoardJson);
        },
      );

      await _openCreation(tester);

      expect(find.text('feat/source'), findsOneWidget);
      expect(find.byType(CreateTaskScreen), findsNothing);
      expect(find.textContaining('프로젝트를 불러오지 못했습니다'), findsOneWidget);

      await _openCreation(tester);

      expect(loads, 2);
      expect(find.byType(CreateTaskScreen), findsOneWidget);
    });

    testWidgets('캐시된 보드 뒤에 프로젝트가 지워지면 새로 조회한 결과로 막는다', (tester) async {
      var loads = 0;
      await _pumpCreationDetail(
        tester,
        loadBoard: () async {
          loads += 1;
          return BoardSnapshot.fromJson(
            loads == 1 ? _creationBoardJson : const {'tasks': {}},
          );
        },
      );
      final container = ProviderScope.containerOf(
        tester.element(find.byType(TaskDetailScreen)),
      );
      await container.read(boardProvider.future);

      await _openCreation(tester);

      expect(loads, 2);
      expect(find.byType(CreateTaskScreen), findsNothing);
      expect(find.textContaining('원본 프로젝트를 찾을 수 없습니다'), findsOneWidget);
    });

    testWidgets('최신 보드를 기다리는 동안 반복해서 눌러도 한 번만 연다', (tester) async {
      final pendingBoard = Completer<BoardSnapshot>();
      var loads = 0;
      await _pumpCreationDetail(
        tester,
        loadBoard: () {
          loads += 1;
          return pendingBoard.future;
        },
      );
      final action = find.bySemanticsIdentifier('create-task-from-current');

      await tester.tap(action);
      await tester.pump();
      expect(find.byTooltip('불러오는 중…'), findsOneWidget);
      expect(tester.getSize(action).height, 48);
      expect(
        tester
            .widget<IconButton>(
              find.descendant(
                of: action,
                matching: find.byWidgetPredicate(
                  (widget) => widget is IconButton,
                ),
              ),
            )
            .onPressed,
        isNull,
      );
      await tester.tap(action);
      await tester.pump();
      expect(loads, 1);

      pendingBoard.complete(BoardSnapshot.fromJson(_creationBoardJson));
      await tester.pumpAndSettle();

      expect(
        find.byType(CreateTaskScreen, skipOffstage: false),
        findsOneWidget,
      );
    });

    testWidgets('보드 조회 중 상세를 떠나면 조회가 끝나도 생성 화면을 열지 않는다', (tester) async {
      final pendingBoard = Completer<BoardSnapshot>();
      await _pumpCreationDetail(tester, loadBoard: () => pendingBoard.future);
      await tester.tap(find.bySemanticsIdentifier('create-task-from-current'));
      await tester.pump();

      await tester.pageBack();
      await tester.pumpAndSettle();
      pendingBoard.complete(BoardSnapshot.fromJson(_creationBoardJson));
      await tester.pumpAndSettle();

      expect(find.text('보드 화면'), findsOneWidget);
      expect(find.byType(CreateTaskScreen), findsNothing);
      expect(tester.takeException(), isNull);
    });

    testWidgets('취소하면 원래 상세와 터미널 구독을 유지한다', (tester) async {
      final client = await _pumpCreationDetail(tester);
      await _openCreation(tester);

      await tester.pageBack();
      await tester.pumpAndSettle();

      expect(find.text('feat/source'), findsOneWidget);
      expect(client.closedPanes, isEmpty);
      expect(client.createCalls, isEmpty);
    });

    testWidgets('생성 오류는 새 입력을 보존하고 취소하면 원래 상세로 돌아간다', (tester) async {
      await _pumpCreationDetail(
        tester,
        createError: const DesktopRequestException(
          400,
          message: '브랜치 이름이 중복됩니다.',
        ),
      );
      await _openCreation(tester);
      await tester.enterText(find.byType(TextFormField).first, 'feat/child');

      await _submitCreation(tester);

      expect(find.text('브랜치 이름이 중복됩니다.'), findsOneWidget);
      expect(
        tester
            .widget<TextFormField>(find.byType(TextFormField).first)
            .controller!
            .text,
        'feat/child',
      );
      await tester.pageBack();
      await tester.pumpAndSettle();
      expect(find.text('feat/source'), findsOneWidget);
    });

    testWidgets('성공하면 새 상세로 교체하고 옛 구독을 닫아 한 번 뒤로 가면 보드다', (tester) async {
      final client = await _pumpCreationDetail(tester);
      await _openCreation(tester);
      await tester.enterText(find.byType(TextFormField).first, 'feat/child');

      await _submitCreation(tester);

      expect(find.text('feat/child'), findsOneWidget);
      expect(
        find.byType(TaskDetailScreen, skipOffstage: false),
        findsOneWidget,
      );
      expect(client.closedPanes, contains('%19'));
      await tester.pageBack();
      await tester.pumpAndSettle();
      expect(find.text('보드 화면'), findsOneWidget);
      expect(find.byType(TaskDetailScreen, skipOffstage: false), findsNothing);
    });

    for (final size in [
      const Size(320, 844),
      phoneSize,
      const Size(844, 390),
    ]) {
      testWidgets(
        '${size.width}×${size.height}에서 긴 프로젝트명과 PR이 있어도 생성 버튼이 48dp다',
        (tester) async {
          await _pumpCreationDetail(
            tester,
            size: size,
            task: const BoardTask(
              id: 'source-task',
              title: '원본 태스크',
              status: TaskStatus.progress,
              projectId: 'project-1',
              projectName: 'very-long-project-name-긴-프로젝트-이름-끝까지',
              branchName: 'feat/source',
              prUrl: 'https://github.com/example/kanvibe/pull/1',
            ),
          );

          final action = find.bySemanticsIdentifier('create-task-from-current');
          final bounds = tester.getRect(action);
          expect(bounds.height, greaterThanOrEqualTo(48));
          expect(bounds.left, greaterThanOrEqualTo(0));
          expect(bounds.right, lessThanOrEqualTo(size.width));
          expect(
            find.descendant(
              of: find.byType(AppBar),
              matching: find.byType(TerminalChromeHeader),
            ),
            findsOneWidget,
          );
          expect(find.byTooltip('PR 열기'), findsOneWidget);
          expect(tester.takeException(), isNull);

          await _openCreation(tester);

          expect(find.byType(CreateTaskScreen), findsOneWidget);
        },
      );
    }
  });

  testWidgets('폰에서는 모든 window의 pane이 각각 탭이 된다', (tester) async {
    await pumpDetail(tester, size: phoneSize);

    /// window 두 개에 걸친 pane 세 개가 전부 탭으로 나와야 한다
    expect(find.text('claude'), findsOneWidget);
    expect(find.text('zsh'), findsOneWidget);
    expect(find.text('bash'), findsOneWidget);
  });

  testWidgets('폰에서는 고른 pane 하나만 화면에 보인다', (tester) async {
    await pumpDetail(tester, size: phoneSize);

    expect(find.byType(PaneTerminalView), findsOneWidget);
  });

  testWidgets('폰에서 다른 탭을 고르면 그 pane으로 바뀐다', (tester) async {
    final client = await pumpDetail(tester, size: phoneSize);
    expect(client.openedPanes.keys, ['%19']);

    /// pane이 많으면 탭 바가 옆으로 넘치므로 사용자처럼 밀어서 꺼낸 뒤 누른다
    await tester.ensureVisible(find.text('bash'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('bash'));
    await tester.pumpAndSettle();

    expect(client.openedPanes.keys, contains('%21'));
    expect(client.closedPanes, contains('%19'), reason: '떠난 pane의 구독은 닫혀야 한다');
  });

  testWidgets('넓은 화면에서도 pane별 탭과 선택한 pane 하나만 보인다', (tester) async {
    await pumpDetail(tester, size: tabletSize);
    expect(find.text('claude'), findsOneWidget);
    expect(find.text('zsh'), findsOneWidget);
    expect(find.byType(PaneTerminalView), findsOneWidget);
  });

  testWidgets('넓은 화면에서 다른 pane 탭을 고르면 입력 대상도 바뀐다', (tester) async {
    final client = await pumpDetail(tester, size: tabletSize);
    await tester.tap(find.text('zsh'));
    await tester.pumpAndSettle();
    expect(find.byType(PaneTerminalView), findsOneWidget);
    terminalIn(tester, '%20').textInput('ls');
    expect(client.writtenInput['%20'], contains('ls'));
    expect(client.closedPanes, contains('%19'));
  });

  testWidgets('화면을 떠나면 pane 구독이 닫힌다', (tester) async {
    final client = await pumpDetail(tester, size: phoneSize);
    expect(client.closedPanes, isEmpty);

    await tester.pumpWidget(
      wrapWithApp(const SizedBox.shrink(), client: client, size: phoneSize),
    );
    await tester.pumpAndSettle();

    expect(client.closedPanes, contains('%19'));
  });

  testWidgets('세션이 꺼져 있으면 빈 터미널 대신 사유와 재시도를 보여 준다', (tester) async {
    await pumpDetail(
      tester,
      size: phoneSize,
      surfaces: null,
      surfacesError: const SurfaceUnavailableException(
        SurfaceUnavailableReason.sessionNotRunning,
      ),
    );

    expect(find.textContaining('세션이 실행 중이 아닙니다'), findsOneWidget);
    expect(find.text('다시 시도'), findsOneWidget);
    expect(find.byType(PaneTerminalView), findsNothing);
  });

  testWidgets('원격 호스트에 닿지 못하면 세션이 아니라 연결을 확인하라고 알려 준다', (tester) async {
    await pumpDetail(
      tester,
      size: phoneSize,
      surfaces: null,
      surfacesError: const SurfaceUnavailableException(
        SurfaceUnavailableReason.remoteUnreachable,
      ),
    );

    expect(find.textContaining('원격 호스트에 연결하지 못했습니다'), findsOneWidget);
    expect(find.textContaining('세션이 실행 중이 아닙니다'), findsNothing);
    expect(find.text('다시 시도'), findsOneWidget);
  });

  testWidgets('zellij 버전이 낮으면 필요한 버전을 알려 준다', (tester) async {
    await pumpDetail(
      tester,
      size: phoneSize,
      surfaces: null,
      surfacesError: const SurfaceUnavailableException(
        SurfaceUnavailableReason.zellijTooOld,
      ),
    );

    expect(find.textContaining('zellij 0.44 이상'), findsOneWidget);
  });

  testWidgets('pane이 하나도 없으면 세션이 꺼진 것으로 안내한다', (tester) async {
    await pumpDetail(
      tester,
      size: phoneSize,
      surfaces: const TaskSurfaces(tabs: []),
    );

    expect(find.textContaining('세션이 실행 중이 아닙니다'), findsOneWidget);
  });

  group('데스크탑 태스크 화면에서 옮겨 온 것', () {
    testWidgets('프로젝트 배지와 브랜치 및 새 태스크는 정보 버튼과 같은 AppBar에 있다', (tester) async {
      await pumpDetail(tester, size: phoneSize);

      final appBar = find.byType(AppBar);
      for (final target in [
        find.text('kanvibe-repo'),
        find.text('feat/kanvibe-mobile'),
        find.bySemanticsIdentifier('create-task-from-current'),
        find.byTooltip('정보'),
      ]) {
        expect(find.descendant(of: appBar, matching: target), findsOneWidget);
      }
      expect(find.text('출력 수신 중'), findsNothing);
      expect(find.byTooltip('붙여넣기'), findsNothing);
    });

    testWidgets('정보를 누르면 데스크탑 정보 패널과 같은 칸이 열린다', (tester) async {
      await pumpDetail(tester, size: phoneSize);

      await tester.tap(find.byTooltip('정보'));
      await tester.pumpAndSettle();

      expect(find.text('브랜치'), findsOneWidget);
      expect(find.text('feat/kanvibe-mobile'), findsNWidgets(2));
      expect(find.text('베이스 브랜치'), findsOneWidget);
      expect(find.text('dev'), findsOneWidget);
    });

    testWidgets('PR 링크가 없는 태스크에는 PR 버튼이 없다', (tester) async {
      await pumpDetail(tester, size: phoneSize);

      expect(find.byTooltip('PR 열기'), findsNothing);
    });

    testWidgets('PR 링크가 있으면 PR 버튼이 생긴다', (tester) async {
      await pumpDetail(
        tester,
        size: phoneSize,
        task: const BoardTask(
          id: 'task-1',
          title: '모바일 클라이언트',
          status: TaskStatus.progress,
          prUrl: 'https://github.com/rookedsysc/kanvibe/pull/389',
        ),
      );

      expect(find.byTooltip('PR 열기'), findsOneWidget);
    });
  });

  group('상태 옮기기', () {
    testWidgets('지금 상태는 옮길 후보에서 빠지고 Pending도 나오지 않는다', (tester) async {
      await pumpDetail(tester, size: phoneSize);

      await tester.tap(find.byTooltip('상태 옮기기'));
      await tester.pumpAndSettle();

      expect(find.text('Progress(으)로 옮기기'), findsNothing, reason: '지금 상태다');
      expect(
        find.text('Pending(으)로 옮기기'),
        findsNothing,
        reason: '데스크탑도 내놓지 않는다',
      );
      for (final label in ['Todo', 'Review', 'Done']) {
        expect(find.text('$label(으)로 옮기기'), findsOneWidget);
      }
    });

    testWidgets('고른 상태를 데스크탑에 보내고 화면의 태스크도 그 상태가 된다', (tester) async {
      final client = await pumpDetail(tester, size: phoneSize);

      await tester.tap(find.byTooltip('상태 옮기기'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Review(으)로 옮기기'));
      await tester.pumpAndSettle();

      expect(client.statusCalls.single.taskId, 'task-1');
      expect(client.statusCalls.single.status, TaskStatus.review);

      /// 시트를 다시 열면 이제 Review가 후보에서 빠져 있어야 화면이 새 상태를 든 것이다
      await tester.tap(find.byTooltip('상태 옮기기'));
      await tester.pumpAndSettle();
      expect(find.text('Review(으)로 옮기기'), findsNothing);
      expect(find.text('Progress(으)로 옮기기'), findsOneWidget);
    });

    testWidgets('데스크탑이 거절하면 시트를 닫지 않고 사유를 남긴다', (tester) async {
      await pumpDetail(
        tester,
        size: phoneSize,
        statusError: const DesktopRequestException(404),
      );

      await tester.tap(find.byTooltip('상태 옮기기'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Done(으)로 옮기기'));
      await tester.pumpAndSettle();

      expect(find.text('데스크탑에서 이미 지워진 태스크입니다.'), findsOneWidget);
      expect(
        find.text('Done(으)로 옮기기'),
        findsOneWidget,
        reason: '시트가 열린 채여야 다시 고를 수 있다',
      );
    });
  });

  group('세션 종류 변경', () {
    const tmuxSurfaces = TaskSurfaces(
      tabs: [_twoPaneWindow],
      canResize: true,
      sessionType: SessionType.tmux,
    );

    Future<void> chooseZellij(WidgetTester tester) async {
      await tester.tap(find.byTooltip('세션 종류 바꾸기'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('zellij(으)로 바꾸기'));
      await tester.pumpAndSettle();
    }

    testWidgets('데스크탑이 세션 종류를 바꿔 스트림을 끊으면 오류 없이 새 세션 pane에 다시 붙는다', (
      tester,
    ) async {
      final client = await pumpDetail(
        tester,
        size: phoneSize,
        surfaces: tmuxSurfaces,
      );
      client.surfaces = const TaskSurfaces(
        tabs: [_secondWindow],
        sessionType: SessionType.zellij,
      );
      client.paneCloseCodes['%19'] = taskSessionChangedCloseCode;

      await client.openedPanes['%19']!.close();
      await tester.pumpAndSettle();

      expect(client.openedPanes.keys, contains('%21'));
      expect(find.textContaining('연결이 끊겼습니다'), findsNothing);
      expect(find.textContaining('터미널 스트림에 오류'), findsNothing);
    });

    testWidgets('새 세션의 pane id가 이전과 같아도 스트림을 새로 연다', (tester) async {
      final client = await pumpDetail(
        tester,
        size: phoneSize,
        surfaces: tmuxSurfaces,
      );
      final previousStream = client.openedPanes['%19']!;
      client.surfaces = const TaskSurfaces(
        tabs: [_twoPaneWindow],
        sessionType: SessionType.zellij,
      );
      client.paneCloseCodes['%19'] = taskSessionChangedCloseCode;

      await previousStream.close();
      await tester.pumpAndSettle();

      expect(client.openedPanes['%19'], isNot(same(previousStream)));
      expect(find.textContaining('연결이 끊겼습니다'), findsNothing);
    });

    testWidgets('기존 세션이 켜져 있으면 닫을지 묻고 닫기를 고르면 닫으라고 보낸 뒤 시트를 닫는다', (
      tester,
    ) async {
      final client = await pumpDetail(
        tester,
        size: phoneSize,
        surfaces: tmuxSurfaces,
      );

      await tester.tap(find.byTooltip('세션 종류 바꾸기'));
      await tester.pumpAndSettle();
      expect(find.text('지금은 tmux입니다.'), findsOneWidget);
      expect(find.text('tmux(으)로 바꾸기'), findsNothing);
      await tester.tap(find.text('zellij(으)로 바꾸기'));
      await tester.pumpAndSettle();

      expect(find.text('기존 세션 닫기'), findsOneWidget);
      await tester.tap(find.text('닫기'));
      await tester.pumpAndSettle();

      expect(client.sessionTypeChangeCalls.single, (
        taskId: 'task-1',
        sessionType: SessionType.zellij,
        shouldCloseExistingSession: true,
      ));
      expect(find.text('지금은 tmux입니다.'), findsNothing);
    });

    testWidgets('남겨 두기를 고르면 기존 세션을 닫지 않고 바꾼다', (tester) async {
      final client = await pumpDetail(
        tester,
        size: phoneSize,
        surfaces: tmuxSurfaces,
      );

      await chooseZellij(tester);
      await tester.tap(find.text('남겨 두기'));
      await tester.pumpAndSettle();

      expect(
        client.sessionTypeChangeCalls.single.shouldCloseExistingSession,
        isFalse,
      );
    });

    testWidgets('기존 세션이 꺼져 있으면 묻지 않고 바꾼다', (tester) async {
      final client = FakeDesktopClient(surfaces: tmuxSurfaces)
        ..isTaskSessionAlive = false;
      await pumpDetail(tester, size: phoneSize, desktopClient: client);

      await chooseZellij(tester);

      expect(find.text('기존 세션 닫기'), findsNothing);
      expect(client.sessionTypeChangeCalls.single, (
        taskId: 'task-1',
        sessionType: SessionType.zellij,
        shouldCloseExistingSession: false,
      ));
    });

    testWidgets('닫기 확인 창을 그냥 닫으면 바꾸지 않고 다시 고를 수 있다', (tester) async {
      final client = await pumpDetail(
        tester,
        size: phoneSize,
        surfaces: tmuxSurfaces,
      );

      await chooseZellij(tester);
      await tester.tapAt(const Offset(5, 5));
      await tester.pumpAndSettle();

      expect(client.sessionTypeChangeCalls, isEmpty);
      final retryButton = tester.widget<OutlinedButton>(
        find.widgetWithText(OutlinedButton, 'zellij(으)로 바꾸기'),
      );
      expect(retryButton.onPressed, isNotNull);
    });

    testWidgets('데스크탑에 새 세션 도구가 없으면 그 사유를 시트에 남긴다', (tester) async {
      final client = FakeDesktopClient(surfaces: tmuxSurfaces)
        ..isTaskSessionAlive = false
        ..sessionTypeChangeError = const DesktopRequestException(
          409,
          message: 'zellij이(가) 설치되어 있지 않습니다. 데스크탑에서 설치한 뒤 다시 시도해 주세요.',
        );
      await pumpDetail(tester, size: phoneSize, desktopClient: client);

      await chooseZellij(tester);

      expect(find.textContaining('데스크탑에서 설치한 뒤'), findsOneWidget);
      expect(find.text('지금은 tmux입니다.'), findsOneWidget);
    });
  });

  group('삭제', () {
    /// 삭제가 끝나면 상세 화면이 닫혀 보드로 돌아가야 하므로, 보드 자리에서 상세 화면을 띄운다
    Future<FakeDesktopClient> pumpDetailFromBoard(
      WidgetTester tester, {
      List<Object?> deleteErrors = const [],
      int failedPeerCount = 0,
    }) async {
      await tester.binding.setSurfaceSize(phoneSize);
      final client = FakeDesktopClient(
        surfaces: _surfaces,
        deleteErrors: deleteErrors,
        failedPeerCount: failedPeerCount,
      );
      await tester.pumpWidget(
        wrapWithApp(
          Builder(
            builder: (context) => Scaffold(
              body: Center(
                child: TextButton(
                  onPressed: () => Navigator.of(context).push(
                    MaterialPageRoute<void>(
                      builder: (_) => const TaskDetailScreen(task: _task),
                    ),
                  ),
                  child: const Text('보드'),
                ),
              ),
            ),
          ),
          client: client,
          size: phoneSize,
        ),
      );
      await tester.tap(find.text('보드'));
      await tester.pumpAndSettle();
      await tester.tap(find.byTooltip('상태 옮기기'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('삭제'));
      await tester.pumpAndSettle();
      return client;
    }

    testWidgets('상태 시트의 삭제는 한 번 더 묻고, 취소하면 아무것도 지우지 않는다', (tester) async {
      final client = await pumpDetailFromBoard(tester);

      expect(find.text('이 작업을 삭제하시겠습니까?'), findsOneWidget);
      await tester.tap(find.text('취소'));
      await tester.pumpAndSettle();

      expect(client.deleteCalls, isEmpty);
      expect(find.byType(TaskDetailScreen), findsOneWidget);
    });

    testWidgets('확인하면 변경을 지키는 기본 삭제를 보내고 보드로 돌아간다', (tester) async {
      final client = await pumpDetailFromBoard(tester);

      await tester.tap(find.widgetWithText(TextButton, '삭제').last);
      await tester.pumpAndSettle();

      expect(client.deleteCalls, [(taskId: 'task-1', isForced: false)]);
      expect(find.byType(TaskDetailScreen), findsNothing);
      expect(find.text('보드'), findsOneWidget);
      expect(find.byType(SnackBar), findsNothing);
    });

    testWidgets('꺼져 있던 다른 데스크탑에는 남았다고 보드에서 알린다', (tester) async {
      await pumpDetailFromBoard(tester, failedPeerCount: 1);

      await tester.tap(find.widgetWithText(TextButton, '삭제').last);
      await tester.pumpAndSettle();

      expect(find.byType(TaskDetailScreen), findsNothing);
      expect(
        find.text('다른 데스크탑 1곳에는 아직 남아 있습니다. 그 데스크탑이 켜지면 직접 삭제해 주세요.'),
        findsOneWidget,
      );
    });

    testWidgets('커밋하지 않은 변경 때문에 거절되면 데스크탑처럼 강제 삭제를 고를 수 있다', (tester) async {
      final client = await pumpDetailFromBoard(
        tester,
        deleteErrors: const [
          DesktopCommandFailure('dirty-worktree', '커밋하지 않은 변경이 있습니다'),
        ],
      );

      await tester.tap(find.widgetWithText(TextButton, '삭제').last);
      await tester.pumpAndSettle();
      expect(find.text('커밋하지 않은 변경이 있습니다'), findsOneWidget);
      await tester.tap(find.text('강제 삭제'));
      await tester.pumpAndSettle();

      expect(client.deleteCalls, [
        (taskId: 'task-1', isForced: false),
        (taskId: 'task-1', isForced: true),
      ]);
      expect(find.byType(TaskDetailScreen), findsNothing);
    });

    testWidgets('강제 삭제를 취소하면 시트에 남아 아무것도 더 지우지 않는다', (tester) async {
      final client = await pumpDetailFromBoard(
        tester,
        deleteErrors: const [
          DesktopCommandFailure('dirty-worktree', '커밋하지 않은 변경이 있습니다'),
        ],
      );

      await tester.tap(find.widgetWithText(TextButton, '삭제').last);
      await tester.pumpAndSettle();
      await tester.tap(find.text('취소'));
      await tester.pumpAndSettle();

      expect(client.deleteCalls, [(taskId: 'task-1', isForced: false)]);
      expect(find.byType(TaskDetailScreen), findsOneWidget);
    });

    testWidgets('깨진 저장소는 동의 후 등록만 제거하고 상세를 닫는다', (tester) async {
      final client = await pumpDetailFromBoard(
        tester,
        deleteErrors: const [
          DesktopCommandFailure('invalid-worktree', 'Git 저장소를 찾을 수 없습니다'),
        ],
      );
      await tester.tap(find.widgetWithText(TextButton, '삭제').last);
      await tester.pumpAndSettle();
      expect(find.text('태스크 등록만 제거할까요?'), findsOneWidget);
      expect(client.removedRegistrationIds, isEmpty);
      await tester.tap(find.widgetWithText(TextButton, '등록만 제거'));
      await tester.pumpAndSettle();
      expect(client.removedRegistrationIds, ['task-1']);
      expect(client.deleteCalls, [(taskId: 'task-1', isForced: false)]);
      expect(find.byType(TaskDetailScreen), findsNothing);
    });

    testWidgets('상세에서 등록 제거를 취소하면 태스크를 유지한다', (tester) async {
      final client = await pumpDetailFromBoard(
        tester,
        deleteErrors: const [
          DesktopCommandFailure('invalid-worktree', 'Git 저장소를 찾을 수 없습니다'),
        ],
      );
      await tester.tap(find.widgetWithText(TextButton, '삭제').last);
      await tester.pumpAndSettle();
      expect(find.text('태스크 등록만 제거할까요?'), findsOneWidget);
      await tester.tap(find.text('취소'));
      await tester.pumpAndSettle();
      expect(client.removedRegistrationIds, isEmpty);
      expect(find.byType(TaskDetailScreen), findsOneWidget);
    });

    testWidgets('그 밖의 실패는 시트를 닫지 않고 데스크탑이 알려 준 사유를 남긴다', (tester) async {
      await pumpDetailFromBoard(
        tester,
        deleteErrors: const [DesktopCommandFailure('server-error', '서버 오류')],
      );

      await tester.tap(find.widgetWithText(TextButton, '삭제').last);
      await tester.pumpAndSettle();

      expect(find.text('서버 오류'), findsOneWidget);
      expect(find.byType(TaskDetailScreen), findsOneWidget);
    });

    testWidgets('이미 지워진 태스크는 삭제된 것으로 보고 보드로 돌아간다', (tester) async {
      await pumpDetailFromBoard(
        tester,
        deleteErrors: const [
          DesktopCommandFailure('task-not-found', '없는 태스크입니다'),
        ],
      );

      await tester.tap(find.widgetWithText(TextButton, '삭제').last);
      await tester.pumpAndSettle();

      expect(find.byType(TaskDetailScreen), findsNothing);
    });

    testWidgets('데스크탑이 명령을 모르면 업데이트 안내를 시트에 남긴다', (tester) async {
      await pumpDetailFromBoard(
        tester,
        deleteErrors: const [DesktopUpdateRequiredException()],
      );

      await tester.tap(find.widgetWithText(TextButton, '삭제').last);
      await tester.pumpAndSettle();

      expect(find.text('데스크탑 앱을 업데이트해 주세요'), findsOneWidget);
      expect(find.byType(TaskDetailScreen), findsOneWidget);
    });

    testWidgets('연결이 끊겨 있으면 끊김 안내를 시트에 남긴다', (tester) async {
      await pumpDetailFromBoard(
        tester,
        deleteErrors: const [DesktopDisconnectedException()],
      );

      await tester.tap(find.widgetWithText(TextButton, '삭제').last);
      await tester.pumpAndSettle();

      expect(find.text('데스크탑과 연결이 끊겼습니다. 연결을 확인해 주세요.'), findsOneWidget);
      expect(find.byType(TaskDetailScreen), findsOneWidget);
    });
  });
  group('탭 닫기', () {
    Finder closeButton(String paneId) =>
        find.bySemanticsIdentifier('close-terminal-tab-$paneId');

    /// 탭 하나가 pane 하나인 목록의 서버 응답 모양
    Map<String, dynamic> closedResult(List<String> remainingPaneIds) => {
      'ok': true,
      'surfaces': {
        'sessionType': 'tmux',
        'canResize': true,
        'tabs': [
          for (final paneId in remainingPaneIds)
            {
              'id': 'tab$paneId',
              'panes': [
                {'id': paneId, 'width': 80, 'height': 24, 'command': 'zsh'},
              ],
            },
        ],
      },
    };

    Future<FakeDesktopClient> pumpClosable(
      WidgetTester tester, {
      required Future<Map<String, dynamic>> Function(DesktopCommand command)
      onRunCommand,
      TaskSurfaces surfaces = _surfaces,
    }) async {
      final client = FakeDesktopClient(surfaces: surfaces)
        ..onRunCommand = onRunCommand;
      await pumpDetail(tester, size: phoneSize, desktopClient: client);
      return client;
    }

    String selectedPaneId(WidgetTester tester) =>
        tester.widget<PaneTerminalView>(find.byType(PaneTerminalView)).pane.id;

    testWidgets('유휴 셸은 확인 없이 닫고 선택은 다음 탭으로 넘어간다', (tester) async {
      final client = await pumpClosable(
        tester,
        onRunCommand: (command) async => command.name == 'get-tab-close-info'
            ? {'ok': true, 'processNames': <String>[]}
            : closedResult(['%20', '%21']),
      );

      await tester.tap(closeButton('%19'));
      await tester.pumpAndSettle();

      expect(find.byType(AlertDialog), findsNothing);
      expect(client.commandCalls.map((command) => command.name), [
        'get-tab-close-info',
        'close-tab',
      ]);
      expect(client.commandCalls.last.fields, {
        'taskId': 'task-1',
        'paneId': '%19',
      });
      expect(selectedPaneId(tester), '%20');
      expect(closeButton('%19'), findsNothing);
    });

    testWidgets('선택한 탭을 닫으면 앞 탭이 선택된다', (tester) async {
      await pumpClosable(
        tester,
        onRunCommand: (command) async => command.name == 'get-tab-close-info'
            ? {'ok': true, 'processNames': <String>[]}
            : closedResult(['%19', '%21']),
      );
      await tester.tap(find.bySemanticsIdentifier('terminal-tab-%20'));
      await tester.pumpAndSettle();
      expect(selectedPaneId(tester), '%20');

      await tester.tap(closeButton('%20'));
      await tester.pumpAndSettle();

      expect(selectedPaneId(tester), '%19');
    });

    testWidgets('프로세스가 돌고 있으면 이름을 보여 주고, 취소하면 닫지 않는다', (tester) async {
      final client = await pumpClosable(
        tester,
        onRunCommand: (command) async => {
          'ok': true,
          'processNames': ['vim'],
        },
      );

      await tester.tap(closeButton('%20'));
      await tester.pumpAndSettle();
      expect(find.text('vim 실행 중인 탭을 닫으면 종료됩니다'), findsOneWidget);
      await tester.tap(find.text('취소'));
      await tester.pumpAndSettle();

      expect(client.commandCalls.map((command) => command.name), [
        'get-tab-close-info',
      ]);
      expect(closeButton('%20'), findsOneWidget);
    });

    testWidgets('프로세스가 돌고 있어도 확인하면 닫는다', (tester) async {
      final client = await pumpClosable(
        tester,
        onRunCommand: (command) async => command.name == 'get-tab-close-info'
            ? {
                'ok': true,
                'processNames': ['vim'],
              }
            : closedResult(['%19', '%21']),
      );

      await tester.tap(closeButton('%20'));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(TextButton, '닫기'));
      await tester.pumpAndSettle();

      expect(client.commandCalls.map((command) => command.name), [
        'get-tab-close-info',
        'close-tab',
      ]);
      expect(closeButton('%20'), findsNothing);
    });

    testWidgets('프로세스 조회가 실패하면 확인을 거친다', (tester) async {
      final client = await pumpClosable(
        tester,
        onRunCommand: (command) async =>
            throw const DesktopCommandFailure('server-error', '서버 오류'),
      );

      await tester.tap(closeButton('%20'));
      await tester.pumpAndSettle();

      expect(find.byType(AlertDialog), findsOneWidget);
      await tester.tap(find.text('취소'));
      await tester.pumpAndSettle();
      expect(client.commandCalls.map((command) => command.name), [
        'get-tab-close-info',
      ]);
    });

    testWidgets('탭이 하나뿐이면 닫기 버튼이 눌리지 않는다', (tester) async {
      final client = await pumpClosable(
        tester,
        surfaces: const TaskSurfaces(
          tabs: [
            MirrorTab(
              id: '@10',
              name: 'kanvibe',
              panes: [
                MirrorPane(
                  id: '%19',
                  tabId: '@10',
                  tabName: 'kanvibe',
                  command: 'zsh',
                  left: 0,
                  top: 0,
                  width: 80,
                  height: 24,
                ),
              ],
            ),
          ],
        ),
        onRunCommand: (command) async => throw StateError('보내면 안 된다'),
      );

      await tester.tap(closeButton('%19'), warnIfMissed: false);
      await tester.pumpAndSettle();

      expect(client.commandCalls, isEmpty);
      final button = tester.widget<IconButton>(
        find.descendant(
          of: closeButton('%19'),
          matching: find.byType(IconButton),
        ),
      );
      expect(button.onPressed, isNull);
    });

    testWidgets('닫기 버튼은 최소 터치 크기를 지킨다', (tester) async {
      await pumpClosable(
        tester,
        onRunCommand: (command) async => throw StateError('보내면 안 된다'),
      );

      final size = tester.getSize(closeButton('%19'));

      expect(size.width, greaterThanOrEqualTo(48));
      expect(size.height, greaterThanOrEqualTo(48));
    });

    testWidgets('마지막 탭이라고 거절되면 그 사유를 알리고 탭을 그대로 둔다', (tester) async {
      await pumpClosable(
        tester,
        onRunCommand: (command) async => command.name == 'get-tab-close-info'
            ? {'ok': true, 'processNames': <String>[]}
            : throw const DesktopCommandFailure('last-tab', '마지막 탭은 닫을 수 없습니다'),
      );

      await tester.tap(closeButton('%19'));
      await tester.pumpAndSettle();

      expect(find.text('마지막 탭은 닫을 수 없습니다'), findsOneWidget);
      expect(closeButton('%19'), findsOneWidget);
    });

    testWidgets('이미 닫힌 탭은 성공으로 보고 목록을 다시 받아 안내 없이 끝낸다', (tester) async {
      await pumpClosable(
        tester,
        onRunCommand: (command) async => command.name == 'get-tab-close-info'
            ? {'ok': true, 'processNames': <String>[]}
            : throw const DesktopCommandFailure(
                'pane-not-found',
                '이미 닫혔거나 이 태스크에 없는 탭입니다',
              ),
      );

      await tester.tap(closeButton('%19'));
      await tester.pumpAndSettle();

      expect(find.byType(SnackBar), findsNothing);
    });

    testWidgets('데스크탑이 명령을 모르면 보내지 않고 업데이트 안내를 띄운다', (tester) async {
      final client = await pumpClosable(
        tester,
        onRunCommand: (command) async =>
            throw const DesktopUpdateRequiredException(),
      );

      await tester.tap(closeButton('%19'));
      await tester.pumpAndSettle();

      expect(find.text('데스크탑 앱을 업데이트해 주세요'), findsOneWidget);
      expect(find.byType(AlertDialog), findsNothing);
      expect(client.commandCalls.map((command) => command.name), [
        'get-tab-close-info',
      ]);
    });

    testWidgets('연결이 끊겨 있으면 끊김 안내를 띄운다', (tester) async {
      await pumpClosable(
        tester,
        onRunCommand: (command) async =>
            throw const DesktopDisconnectedException(),
      );

      await tester.tap(closeButton('%19'));
      await tester.pumpAndSettle();

      expect(find.text('데스크탑과 연결이 끊겼습니다. 연결을 확인해 주세요.'), findsOneWidget);
    });

    testWidgets('닫기 응답을 기다리는 동안 고른 다른 탭을 유지한다', (tester) async {
      final pending = Completer<Map<String, dynamic>>();
      await pumpClosable(
        tester,
        onRunCommand: (command) => command.name == 'get-tab-close-info'
            ? Future.value({'ok': true, 'processNames': <String>[]})
            : pending.future,
      );

      await tester.tap(closeButton('%19'));
      await tester.pump();
      await tester.pump();
      await tester.ensureVisible(find.text('bash'));
      await tester.pump();
      await tester.tap(find.text('bash'));
      await tester.pump();
      expect(selectedPaneId(tester), '%21');

      pending.complete(closedResult(['%20', '%21']));
      await tester.pumpAndSettle();

      expect(selectedPaneId(tester), '%21');
      expect(closeButton('%19'), findsNothing);
    });

    testWidgets('닫는 중 반복해 눌러도 닫기 명령은 한 번만 나간다', (tester) async {
      final pending = Completer<Map<String, dynamic>>();
      final client = await pumpClosable(
        tester,
        onRunCommand: (command) => command.name == 'get-tab-close-info'
            ? Future.value({'ok': true, 'processNames': <String>[]})
            : pending.future,
      );

      await tester.tap(closeButton('%19'));
      await tester.pump();
      await tester.pump();
      await tester.tap(closeButton('%20'));
      await tester.pump();
      pending.complete(closedResult(['%20', '%21']));
      await tester.pumpAndSettle();

      expect(
        client.commandCalls.where((command) => command.name == 'close-tab'),
        hasLength(1),
      );
    });
  });
}
