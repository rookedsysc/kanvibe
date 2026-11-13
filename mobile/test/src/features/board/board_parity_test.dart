import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:kanvibe_mobile/src/features/board/presentation/widgets/task_project_icon.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/board/application/board_pagination_controller.dart';
import 'package:kanvibe_mobile/src/features/board/application/board_project_filter_controller.dart';
import 'package:kanvibe_mobile/src/features/board/application/board_sort_controller.dart';
import 'package:kanvibe_mobile/src/features/board/data/board_snapshot.dart';
import 'package:kanvibe_mobile/src/features/board/domain/board_project.dart';
import 'package:kanvibe_mobile/src/features/board/domain/board_task.dart';
import 'package:kanvibe_mobile/src/features/board/domain/board_view_options.dart';
import 'package:kanvibe_mobile/src/features/board/domain/task_priority.dart';
import 'package:kanvibe_mobile/src/features/board/domain/task_status.dart';
import 'package:kanvibe_mobile/src/features/board/presentation/board_screen.dart';
import 'package:kanvibe_mobile/src/features/board/presentation/task_quick_search_screen.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/task_detail/domain/mirror_pane.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/task_detail_screen.dart';

import '../../../support/fake_desktop.dart';

const project = BoardProject(
  id: 'p',
  name: 'KanVibe',
  defaultBranch: 'main',
  isWorktree: false,
);
Map<String, dynamic> taskRow(
  String id, {
  String projectId = 'p',
  String status = 'done',
  String? branch,
}) => {
  'id': id,
  'title': id,
  'status': status,
  'projectId': projectId,
  'branchName': branch,
};
Map<String, dynamic> fixtureBoard() => {
  'projects': [
    {'id': 'p', 'name': 'KanVibe', 'defaultBranch': 'main'},
    {'id': 'q', 'name': 'Other'},
  ],
  'tasks': {
    'todo': [taskRow('root', status: 'todo', branch: 'main')],
    'done': [taskRow('one'), taskRow('two', projectId: 'q')],
  },
  'doneTotal': 4,
  'doneLimit': 2,
};
ProviderContainer containerFor(
  FakeDesktopClient client, {
  FakeConnectionStore? store,
}) => ProviderContainer(
  overrides: [
    desktopClientProvider.overrideWithValue(client),
    connectionStoreProvider.overrideWithValue(store ?? FakeConnectionStore()),
  ],
);

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUp(() => FlutterSecureStorage.setMockInitialValues({}));

  test(
    'metadata, icons, done totals survive parsing and status/project changes',
    () {
      final snapshot = BoardSnapshot.fromJson({
        'projects': [
          {
            'id': 'p',
            'name': 'KanVibe',
            'iconDataUrl': 'data:image/png;base64,AA==',
          },
        ],
        'tasks': {
          'todo': [
            {
              'id': 'task',
              'title': 'Title',
              'status': 'todo',
              'projectId': 'p',
              'createdAt': '2026-01-02T03:04:05Z',
              'updatedAt': '2026-02-03T03:04:05Z',
              'sessionType': 'tmux',
              'worktreePath': '/worktree',
            },
          ],
        },
        'doneTotal': 42,
        'doneLimit': 20,
      });
      final changed = snapshot.allTasks.single.withStatus(TaskStatus.review);
      expect(changed.createdAt, DateTime.utc(2026, 1, 2, 3, 4, 5));
      expect(changed.updatedAt, DateTime.utc(2026, 2, 3, 3, 4, 5));
      expect(changed.sessionType, 'tmux');
      expect(changed.worktreePath, '/worktree');
      expect(changed.projectIconDataUrl, 'data:image/png;base64,AA==');
      final filtered = snapshot.filterByProjectIds({'p'});
      expect(filtered.doneTotal, 42);
      expect(filtered.doneLimit, 20);
    },
  );

  test('project color survives a matching nested detail response', () {
    final task = BoardTask.fromJson({
      ...taskRow('task'),
      'project': {'id': 'p', 'color': '#C4B5FD'},
    }, const {});
    expect(task?.projectColor, '#C4B5FD');
  });

  test(
    'project color from the board map takes precedence over detail metadata',
    () {
      final task = BoardTask.fromJson(
        {
          ...taskRow('task'),
          'projectColor': '#4285F4',
          'project': {'id': 'p', 'color': '#C4B5FD'},
        },
        const {},
        projectColorsById: const {'p': '#123456'},
      );
      expect(task?.projectColor, '#123456');
    },
  );

  test('project color survives a flat task response', () {
    final task = BoardTask.fromJson({
      ...taskRow('task'),
      'projectColor': '#4285F4',
    }, const {});
    expect(task?.projectColor, '#4285F4');
  });

  test(
    'project color ignores unrelated and malformed nested project metadata',
    () {
      for (final project in [
        {'id': 'other', 'color': '#C4B5FD'},
        {'id': 'p', 'color': 42},
        'invalid',
        null,
      ]) {
        final task = BoardTask.fromJson({
          ...taskRow('task'),
          'project': project,
        }, const {});
        expect(task?.projectColor, isNull);
      }
    },
  );

  test(
    'root classification and inherited priority match desktop semantics',
    () {
      const root = BoardTask(
        id: 'root',
        title: 'Root',
        status: TaskStatus.todo,
        projectId: 'p',
        branchName: 'main',
        priority: TaskPriority.high,
      );
      const ordinary = BoardTask(
        id: 'task',
        title: 'Task',
        status: TaskStatus.todo,
        projectId: 'p',
      );
      const explicit = BoardTask(
        id: 'explicit',
        title: 'Explicit',
        status: TaskStatus.todo,
        projectId: 'p',
        priority: TaskPriority.low,
      );
      expect(isProjectRoot(root, [project]), isTrue);
      expect(isProjectRoot(ordinary, [project]), isFalse);
      expect(
        effectivePriority(ordinary, [root, ordinary], [project]),
        TaskPriority.high,
      );
      expect(
        effectivePriority(explicit, [root, explicit], [project]),
        TaskPriority.low,
      );
      expect(
        isProjectRoot(root, [
          const BoardProject(
            id: 'p',
            name: 'Worktree',
            defaultBranch: 'main',
            isWorktree: true,
          ),
        ]),
        isFalse,
      );
    },
  );

  test('multiple sort keys keep missing values last in either direction', () {
    const tasks = [
      BoardTask(
        id: 'a',
        title: 'Zebra',
        status: TaskStatus.todo,
        priority: TaskPriority.high,
      ),
      BoardTask(
        id: 'b',
        title: 'Alpha',
        status: TaskStatus.todo,
        priority: TaskPriority.high,
      ),
      BoardTask(id: 'c', title: 'First', status: TaskStatus.todo),
    ];
    final sorted = sortBoardTasks(
      tasks,
      [
        const BoardSortKey(BoardSortField.priority, descending: true),
        const BoardSortKey(BoardSortField.title),
      ],
      tasks,
      [],
    );
    expect(sorted.map((task) => task.id), ['b', 'a', 'c']);
  });

  test('fuzzy search covers title, project and branch', () {
    const task = BoardTask(
      id: 'x',
      title: 'Implement feature',
      status: TaskStatus.todo,
      projectName: 'KanVibe',
      branchName: 'fix/mobile',
    );
    expect(fuzzyTaskScore(task, 'impl'), 0);
    expect(fuzzyTaskScore(task, 'kvb'), isNotNull);
    expect(fuzzyTaskScore(task, 'fx/mob'), isNotNull);
    expect(fuzzyTaskScore(task, 'zzzz'), isNull);
  });

  test(
    'pagination uses raw loaded count, deduplicates and retains filters',
    () async {
      final client = FakeDesktopClient(board: fixtureBoard());
      final queries = <Map<String, String>?>[];
      client.onJsonRequest = (method, segments, query, body) async {
        if (segments.first == 'settings') {
          return {
            'settings': {
              'sortPreference': {'keys': []},
            },
          };
        }
        queries.add(query);
        return {
          'tasks': [
            taskRow('two', projectId: 'q'),
            taskRow('three'),
            taskRow('four'),
          ],
          'doneTotal': 4,
        };
      };
      final container = containerFor(client);
      addTearDown(container.dispose);
      await container.read(boardPaginationProvider.future);
      await container
          .read(boardProjectFilterProvider('127.0.0.1:9736').notifier)
          .toggleProject('p');
      await container.read(boardPaginationProvider.notifier).loadMore();
      final state = container.read(boardPaginationProvider).requireValue;
      expect(queries.single?['offset'], '2');
      expect(state.snapshot.tasksIn(TaskStatus.done).map((task) => task.id), [
        'one',
        'two',
        'three',
        'four',
      ]);
      expect(state.snapshot.hasMoreDone, isFalse);
      expect(
        container
            .read(filteredBoardProvider)
            .requireValue
            .tasksIn(TaskStatus.done)
            .map((task) => task.id),
        ['one', 'three', 'four'],
      );
      expect(container.read(boardProjectFilterProvider('127.0.0.1:9736')), {
        'p',
      });
    },
  );

  test('page failure preserves loaded content and supports retry', () async {
    final client = FakeDesktopClient(board: fixtureBoard());
    var fail = true;
    client.onJsonRequest = (method, segments, query, body) async {
      if (fail) throw StateError('offline');
      return {
        'tasks': [taskRow('three'), taskRow('four')],
        'doneTotal': 4,
      };
    };
    final container = containerFor(client);
    addTearDown(container.dispose);
    await container.read(boardPaginationProvider.future);
    await container.read(boardPaginationProvider.notifier).loadMore();
    expect(
      container.read(boardPaginationProvider).requireValue.loadMoreFailed,
      isTrue,
    );
    expect(
      container
          .read(boardPaginationProvider)
          .requireValue
          .snapshot
          .tasksIn(TaskStatus.done),
      hasLength(2),
    );
    fail = false;
    await container.read(boardPaginationProvider.notifier).loadMore();
    expect(
      container
          .read(boardPaginationProvider)
          .requireValue
          .snapshot
          .tasksIn(TaskStatus.done),
      hasLength(4),
    );
  });

  test('PC switch rejects the old in-flight done page', () async {
    const second = DesktopConnection(
      host: 'second',
      port: 9736,
      token: 'second',
    );
    final store = FakeConnectionStore();
    final initial = await store.readRegistry();
    await store.writeRegistry(
      SavedConnections(
        connections: [...initial.connections, second],
        activeId: initial.activeId,
      ),
    );
    final pending = Completer<Map<String, dynamic>>();
    final client = FakeDesktopClient(
      board: fixtureBoard(),
      boardsByHost: {
        'second': {
          'tasks': {
            'done': [taskRow('new-pc')],
          },
          'doneTotal': 1,
        },
      },
    )..onJsonRequest = (method, segments, query, body) => pending.future;
    final container = containerFor(client, store: store);
    addTearDown(container.dispose);
    await container.read(boardPaginationProvider.future);
    final loading = container.read(boardPaginationProvider.notifier).loadMore();
    await container
        .read(connectionControllerProvider.notifier)
        .select(second.id);
    await container.read(boardPaginationProvider.future);
    pending.complete({
      'tasks': [taskRow('old-pc')],
      'doneTotal': 9,
    });
    await loading;
    expect(
      container
          .read(boardPaginationProvider)
          .requireValue
          .snapshot
          .allTasks
          .map((task) => task.id),
      ['new-pc'],
    );
  });

  test('sort settings restore and persist through the PC API', () async {
    final client = FakeDesktopClient();
    var saved = <Map<String, String>>[
      {'field': 'title', 'direction': 'asc'},
    ];
    client.onJsonRequest = (method, segments, query, body) async {
      if (method == 'PATCH') {
        saved = List<Map<String, String>>.from(
          (body!['sortPreference'] as Map)['keys'] as List,
        );
      }
      return {
        'settings': {
          'sortPreference': {'keys': saved},
        },
      };
    };
    final container = containerFor(client);
    addTearDown(container.dispose);
    expect(
      (await container.read(boardSortProvider.future)).single.field,
      BoardSortField.title,
    );
    expect(
      await container.read(boardSortProvider.notifier).save([
        const BoardSortKey(BoardSortField.priority),
        const BoardSortKey(BoardSortField.updatedAt, descending: true),
      ]),
      isTrue,
    );
    container.invalidate(boardSortProvider);
    final restored = await container.read(boardSortProvider.future);
    expect(restored.map((key) => key.field), [
      BoardSortField.priority,
      BoardSortField.updatedAt,
    ]);
    expect(restored.last.descending, isTrue);
  });

  testWidgets('card shows priority inheritance, metadata and kind filtering', (
    tester,
  ) async {
    final board = fixtureBoard();
    final rows = (board['tasks'] as Map)['todo'] as List;
    rows[0]['priority'] = 'high';
    rows.add({
      ...taskRow('child', status: 'todo', branch: 'feature'),
      'description': 'Description preview',
      'agentType': 'codex',
      'sessionType': 'tmux',
    });
    final client = FakeDesktopClient(board: board);
    await tester.pumpWidget(
      wrapWithApp(const BoardScreen(), client: client, size: phoneSize),
    );
    await tester.pumpAndSettle();
    expect(
      find.descendant(
        of: find
            .ancestor(
              of: find.text('Description preview'),
              matching: find.byType(InkWell),
            )
            .first,
        matching: find.text('P1 ↓'),
      ),
      findsOneWidget,
    );
    expect(find.text('Description preview'), findsOneWidget);
    expect(find.text('codex'), findsOneWidget);
    expect(find.text('tmux'), findsOneWidget);
    expect(find.text('Base'), findsOneWidget);
    await tester.tap(find.byType(DropdownButton<BoardTaskKind>));
    await tester.pumpAndSettle();
    await tester.tap(find.text('프로젝트 본체').last);
    await tester.pumpAndSettle();
    expect(find.text('root'), findsOneWidget);
    expect(find.text('child'), findsNothing);
  });

  testWidgets('find navigates to a loaded match outside the viewport', (
    tester,
  ) async {
    final board = {
      'tasks': {
        'todo': [
          for (var i = 0; i < 30; i++)
            taskRow(i == 29 ? 'unique-needle' : 'task-$i', status: 'todo'),
        ],
      },
    };
    final client = FakeDesktopClient(board: board);
    await tester.binding.setSurfaceSize(phoneSize);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await tester.pumpWidget(
      wrapWithApp(const BoardScreen(), client: client, size: phoneSize),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('현재 보드에서 찾기'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'unique-needle');
    await tester.pumpAndSettle();
    expect(
      find
          .byWidgetPredicate(
            (widget) => widget is Text && widget.data == 'unique-needle',
          )
          .hitTestable(),
      findsOneWidget,
    );
    expect(find.text('1/1'), findsOneWidget);
  });

  testWidgets('global search fetches all results then opens latest task data', (
    tester,
  ) async {
    final client =
        FakeDesktopClient(
            tasksById: {
              'older': {
                ...taskRow('older', status: 'todo'),
                'title': 'Latest task title',
                'project': {'id': 'p', 'color': '#C4B5FD'},
              },
            },
          )
          ..onJsonRequest = (method, segments, query, body) async => {
            'tasks': [taskRow('older')],
          };
    BoardTask? selected;
    await tester.pumpWidget(
      wrapWithApp(
        Builder(
          builder: (context) => Scaffold(
            body: TextButton(
              onPressed: () async {
                selected = await Navigator.of(context).push<BoardTask>(
                  MaterialPageRoute(
                    builder: (_) => const TaskQuickSearchScreen(),
                  ),
                );
              },
              child: const Text('Search'),
            ),
          ),
        ),
        client: client,
        size: phoneSize,
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.text('Search'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('older'));
    await tester.pumpAndSettle();
    expect(client.fetchedTasks, ['older']);
    expect(selected?.title, 'Latest task title');
    expect(selected?.projectColor, '#C4B5FD');
  });

  testWidgets(
    'tablet workspace replaces embedded details and releases old pane',
    (tester) async {
      final client = FakeDesktopClient(
        board: {
          'tasks': {
            'todo': [
              taskRow('first', status: 'todo'),
              taskRow('second', status: 'todo'),
            ],
          },
        },
        surfaces: const TaskSurfaces(
          tabs: [
            MirrorTab(
              id: 'tab',
              name: 'tab',
              panes: [
                MirrorPane(
                  id: 'pane',
                  tabId: 'tab',
                  tabName: 'tab',
                  command: 'sh',
                  left: 0,
                  top: 0,
                  width: 80,
                  height: 24,
                ),
              ],
            ),
          ],
        ),
      );
      await tester.binding.setSurfaceSize(tabletSize);
      addTearDown(() => tester.binding.setSurfaceSize(null));
      await tester.pumpWidget(
        wrapWithApp(const BoardScreen(), client: client, size: tabletSize),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byTooltip('Workspace 보기'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('first'));
      await tester.pumpAndSettle();
      expect(
        tester.widget<TaskDetailScreen>(find.byType(TaskDetailScreen)).task.id,
        'first',
      );
      final createButton = find.widgetWithIcon(FloatingActionButton, Icons.add);
      expect(
        tester
            .getRect(createButton)
            .overlaps(tester.getRect(find.byType(TaskDetailScreen))),
        isFalse,
        reason: 'Workspace creation must not cover terminal controls',
      );
      await tester.tap(find.text('second'));
      await tester.pumpAndSettle();
      expect(
        tester.widget<TaskDetailScreen>(find.byType(TaskDetailScreen)).task.id,
        'second',
      );
      expect(client.closedPanes, contains('pane'));
      expect(find.byType(TaskDetailScreen), findsOneWidget);
      await tester.tap(find.byTooltip('태스크 메뉴'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('상태 옮기기'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('삭제'));
      await tester.pumpAndSettle();
      await tester.tap(
        find.descendant(
          of: find.byType(AlertDialog),
          matching: find.text('삭제'),
        ),
      );
      await tester.pumpAndSettle();
      expect(client.deleteCalls.single.taskId, 'second');
      expect(find.byType(BoardScreen), findsOneWidget);
      expect(find.byType(TaskDetailScreen), findsNothing);
      expect(find.text('태스크를 선택해 주세요.'), findsOneWidget);
    },
  );
  testWidgets('PR card opens the external URL without navigating to details', (
    tester,
  ) async {
    final launched = <String>[];
    const channel = MethodChannel('plugins.flutter.io/url_launcher');
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(channel, (
      call,
    ) async {
      if (call.method == 'launch') {
        launched.add((call.arguments as Map)['url'] as String);
      }
      return true;
    });
    addTearDown(
      () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        channel,
        null,
      ),
    );
    final client = FakeDesktopClient(
      board: {
        'tasks': {
          'todo': [
            {
              ...taskRow('pr-task', status: 'todo'),
              'prUrl': 'https://github.com/example/project/pull/12',
            },
          ],
        },
      },
    );
    await tester.pumpWidget(
      wrapWithApp(const BoardScreen(), client: client, size: phoneSize),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.text('PR'));
    await tester.pumpAndSettle();
    expect(launched, ['https://github.com/example/project/pull/12']);
    expect(find.byType(TaskDetailScreen), findsNothing);
  });

  testWidgets('sort UI saves ordered criteria and direction', (tester) async {
    Map<String, dynamic>? written;
    final client = FakeDesktopClient(board: fixtureBoard())
      ..onJsonRequest = (method, segments, query, body) async {
        if (method == 'PATCH') written = body;
        return {
          'settings': {
            'sortPreference': {'keys': []},
          },
        };
      };
    await tester.pumpWidget(
      wrapWithApp(const BoardScreen(), client: client, size: phoneSize),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('보드 정렬'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('정렬 기준 추가'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('제목').last);
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('정렬 방향 바꾸기'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('적용'));
    await tester.pumpAndSettle();
    expect(written, {
      'sortPreference': {
        'keys': [
          {'field': 'title', 'direction': 'desc'},
        ],
      },
    });
  });

  testWidgets(
    'invalid project icon data falls back without an image exception',
    (tester) async {
      await tester.pumpWidget(
        const MaterialApp(
          home: TaskProjectIcon(dataUrl: 'data:image/png;base64,@@@@'),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.byIcon(Icons.folder_outlined), findsOneWidget);
      expect(tester.takeException(), isNull);
    },
  );
}
