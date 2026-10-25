import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/constants/app_theme.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/board/data/board_snapshot.dart';
import 'package:kanvibe_mobile/src/features/board/domain/board_task.dart';
import 'package:kanvibe_mobile/src/features/board/domain/task_status.dart';
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
}) async {
  await tester.binding.setSurfaceSize(size);
  final client = FakeDesktopClient(
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
    expect(find.text('zellij'), findsOneWidget);

    await tester.enterText(find.byType(TextFormField).first, 'feat/child');
    await _submitCreation(tester);

    expect(client.createCalls.single, {
      'projectId': 'project-1',
      'branchName': 'feat/child',
      'baseBranch': 'feat/source',
      'description': '',
      'priority': null,
      'sessionType': 'zellij',
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
        expect(find.text('원본 태스크'), findsOneWidget);
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

      expect(find.text('원본 태스크'), findsOneWidget);
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
      expect(find.text('불러오는 중…'), findsOneWidget);
      expect(tester.getSize(action).height, 48);
      expect(
        tester
            .widget<TextButton>(
              find.descendant(
                of: action,
                matching: find.byWidgetPredicate(
                  (widget) => widget is TextButton,
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

      expect(find.text('원본 태스크'), findsOneWidget);
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
      expect(find.text('원본 태스크'), findsOneWidget);
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
          expect(tester.getSize(find.byType(TerminalChromeHeader)).height, 48);
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
    testWidgets('터미널 위에는 프로젝트만 붙고 태스크 제목은 AppBar에만 있다', (tester) async {
      await pumpDetail(tester, size: phoneSize);

      expect(find.text('kanvibe-repo'), findsOneWidget);
      expect(
        find.text('모바일 클라이언트'),
        findsOneWidget,
        reason: '머리글이 AppBar 제목을 되풀이하지 않는다',
      );
    });

    testWidgets('정보를 누르면 데스크탑 정보 패널과 같은 칸이 열린다', (tester) async {
      await pumpDetail(tester, size: phoneSize);

      await tester.tap(find.byTooltip('정보'));
      await tester.pumpAndSettle();

      expect(find.text('브랜치'), findsOneWidget);
      expect(find.text('feat/kanvibe-mobile'), findsOneWidget);
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
}
