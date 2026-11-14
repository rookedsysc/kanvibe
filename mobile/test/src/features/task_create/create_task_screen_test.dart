import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/board/domain/board_project.dart';
import 'package:kanvibe_mobile/src/features/task_create/presentation/create_task_screen.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/task_detail_screen.dart';

import '../../../support/fake_desktop.dart';

const _projects = [
  BoardProject(
    id: 'p1',
    name: 'kanvibe',
    defaultBranch: 'dev',
    isWorktree: false,
  ),
];

/// 화면을 밀어 올려 띄운다. 만들고 나면 스스로 닫히므로 돌아갈 자리가 있어야 한다
Future<FakeDesktopClient> pumpCreate(
  WidgetTester tester, {
  List<BoardProject> projects = _projects,
  Object? createError,
  Map<String, dynamic> board = const {},
}) async {
  await tester.binding.setSurfaceSize(phoneSize);
  final client = FakeDesktopClient(createError: createError, board: board);

  await tester.pumpWidget(
    wrapWithApp(
      Builder(
        builder: (context) => TextButton(
          onPressed: () => Navigator.of(context).push(
            MaterialPageRoute<void>(
              builder: (_) => CreateTaskScreen(projects: projects),
            ),
          ),
          child: const Text('열기'),
        ),
      ),
      client: client,
      size: phoneSize,
    ),
  );

  await tester.tap(find.text('열기'));
  await tester.pumpAndSettle();
  return client;
}

Future<void> selectProject(
  WidgetTester tester, {
  String name = 'kanvibe',
}) async {
  await tester.tap(find.byType(DropdownButtonFormField<BoardProject>));
  await tester.pumpAndSettle();
  await tester.tap(find.text(name).last);
  await tester.pumpAndSettle();
}

void main() {
  for (final size in [const Size(360, 900), const Size(840, 900)]) {
    testWidgets('긴 프로젝트와 브랜치의 overflow와 중앙 정렬 $size', (tester) async {
      addTearDown(() => tester.binding.setSurfaceSize(null));
      await tester.binding.setSurfaceSize(size);
      final project = BoardProject(
        id: 'long',
        name: 'project-' * 20,
        defaultBranch: 'feature/' * 20,
        isWorktree: false,
      );
      await tester.pumpWidget(
        wrapWithApp(
          CreateTaskScreen(projects: [project], defaultProjectId: project.id),
          client: FakeDesktopClient(),
          size: size,
        ),
      );
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      expect(tester.getCenter(find.byType(Form)).dx, size.width / 2);
      await tester.tap(find.byType(DropdownButtonFormField<BoardProject>));
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('데스크탑 생성 창과 같은 칸을 같은 순서로 묻는다', (tester) async {
    await pumpCreate(tester);

    for (final label in ['프로젝트 *', '베이스 브랜치', '브랜치 이름 *', '설명', '우선순위']) {
      expect(find.text(label), findsOneWidget, reason: '$label 칸이 있어야 한다');
    }
  });

  testWidgets('프로젝트를 고르지 않으면 데스크탑을 부르지 않고 막는다', (tester) async {
    final client = await pumpCreate(tester);

    await tester.enterText(find.byType(TextFormField).first, 'feat/x');
    await tester.tap(find.text('만들기'));
    await tester.pumpAndSettle();

    expect(client.createCalls, isEmpty);
  });

  testWidgets('브랜치 이름이 비면 막고 무엇을 적어야 하는지 말한다', (tester) async {
    final client = await pumpCreate(tester);
    await selectProject(tester);

    await tester.tap(find.text('만들기'));
    await tester.pumpAndSettle();

    expect(find.text('브랜치 이름을 적어 주세요.'), findsOneWidget);
    expect(client.createCalls, isEmpty);
  });

  testWidgets('프로젝트를 고르면 베이스 브랜치가 그 프로젝트의 기본 브랜치로 채워진다', (tester) async {
    await pumpCreate(tester);
    await selectProject(tester);

    expect(find.text('dev'), findsOneWidget);
  });

  testWidgets('다른 프로젝트를 고르면 베이스 브랜치 표시도 새 기본값으로 바뀐다', (tester) async {
    await pumpCreate(
      tester,
      projects: const [
        ..._projects,
        BoardProject(
          id: 'p2',
          name: 'another-project',
          defaultBranch: 'release',
          isWorktree: false,
        ),
      ],
    );
    await selectProject(tester);
    expect(find.text('dev'), findsOneWidget);

    await selectProject(tester, name: 'another-project');

    expect(find.text('release'), findsOneWidget);
    expect(find.text('dev'), findsNothing);
  });

  testWidgets('채운 값을 데스크탑 생성 경로가 쓰는 이름 그대로 보낸다', (tester) async {
    final client = await pumpCreate(tester);
    await selectProject(tester);

    await tester.enterText(find.byType(TextFormField).first, 'feat/새-태스크');
    await tester.enterText(find.byType(TextFormField).last, '무엇을 하는지');
    await tester.tap(find.text('만들기'));
    await tester.pumpAndSettle();

    expect(client.createCalls.single, {
      'projectId': 'p1',
      'branchName': 'feat/새-태스크',
      'baseBranch': 'dev',
      'description': '무엇을 하는지',
      'priority': null,
    });
  });

  testWidgets('세션 종류는 고르지 않고 보내지도 않아 데스크탑 기본값을 따르게 한다', (tester) async {
    final client = await pumpCreate(tester);
    await selectProject(tester);

    expect(find.text('세션 타입'), findsNothing);

    await tester.enterText(find.byType(TextFormField).first, 'feat/x');
    await tester.tap(find.text('만들기'));
    await tester.pumpAndSettle();

    expect(client.createCalls.single, isNot(contains('sessionType')));
  });

  testWidgets('데스크탑이 값을 거절하면 그 사유를 폼에 남긴다', (tester) async {
    await pumpCreate(
      tester,
      createError: const DesktopRequestException(
        400,
        message: '브랜치 이름에 쓸 수 없는 문자가 있습니다',
      ),
    );
    await selectProject(tester);

    await tester.enterText(find.byType(TextFormField).first, 'feat/x');
    await tester.tap(find.text('만들기'));
    await tester.pumpAndSettle();

    expect(find.text('브랜치 이름에 쓸 수 없는 문자가 있습니다'), findsOneWidget);
  });

  testWidgets('데스크탑이 사유를 감추면 어디서 확인할지 알려 준다', (tester) async {
    await pumpCreate(tester, createError: const DesktopRequestException(500));
    await selectProject(tester);

    await tester.enterText(find.byType(TextFormField).first, 'feat/x');
    await tester.tap(find.text('만들기'));
    await tester.pumpAndSettle();

    expect(
      find.text('태스크를 만들지 못했습니다. 데스크탑에서 자세한 사유를 확인해 주세요.'),
      findsOneWidget,
    );
  });

  testWidgets('응답 시간이 넘어도 보드에 같은 브랜치 태스크가 있으면 만든 것으로 보고 닫는다', (tester) async {
    await pumpCreate(
      tester,
      createError: TimeoutException('생성 응답 대기 시간 초과'),
      board: {
        'projects': [
          {'id': 'p1', 'name': 'kanvibe', 'defaultBranch': 'dev'},
        ],
        'tasks': {
          'todo': [
            {
              'id': 'task-late',
              'title': 'feat/x',
              'status': 'todo',
              'projectId': 'p1',
              'branchName': 'feat/x',
            },
          ],
        },
      },
    );
    await selectProject(tester);

    await tester.enterText(find.byType(TextFormField).first, 'feat/x');
    await tester.tap(find.text('만들기'));
    await tester.pumpAndSettle();

    expect(find.byType(CreateTaskScreen), findsNothing);
  });

  testWidgets('응답 시간이 넘고 보드에도 없으면 결과를 확인하지 못했다고 알린다', (tester) async {
    await pumpCreate(
      tester,
      createError: TimeoutException('생성 응답 대기 시간 초과'),
      board: {
        'projects': [
          {'id': 'p1', 'name': 'kanvibe', 'defaultBranch': 'dev'},
        ],
        'tasks': {'todo': []},
      },
    );
    await selectProject(tester);

    await tester.enterText(find.byType(TextFormField).first, 'feat/x');
    await tester.tap(find.text('만들기'));
    await tester.pumpAndSettle();

    expect(find.text('생성 결과를 확인하지 못했습니다. 잠시 뒤 보드에서 확인해 주세요.'), findsOneWidget);
    expect(find.byType(CreateTaskScreen), findsOneWidget);
  });

  group('만드는 중 화면을 떠나면', () {
    /// 보드를 지켜보는 화면 위에 생성 화면을 띄운다. 보드를 다시 읽는지 보려면 누군가 보드를 보고 있어야 한다
    Future<void> pumpOverBoard(
      WidgetTester tester,
      _HeldCreateClient client,
    ) async {
      await tester.binding.setSurfaceSize(phoneSize);
      await tester.pumpWidget(
        wrapWithApp(
          Consumer(
            builder: (context, ref, _) {
              ref.watch(boardProvider);
              // 결과 안내는 화면의 Scaffold에 뜬다. 실제 앱의 보드 화면처럼 Scaffold를 둔다
              return Scaffold(
                body: TextButton(
                  onPressed: () => Navigator.of(context).push(
                    MaterialPageRoute<void>(
                      builder: (_) =>
                          const CreateTaskScreen(projects: _projects),
                    ),
                  ),
                  child: const Text('열기'),
                ),
              );
            },
          ),
          client: client,
          size: phoneSize,
        ),
      );
      await tester.tap(find.text('열기'));
      await tester.pumpAndSettle();
      await selectProject(tester);
      await tester.enterText(find.byType(TextFormField).first, 'feat/x');
      await tester.tap(find.text('만들기'));
      await tester.pump();
    }

    testWidgets('생성은 계속되고 끝나면 보드를 다시 읽고 열기 안내를 띄운다', (tester) async {
      final pending = Completer<void>();
      final client = _HeldCreateClient(pendingCreate: pending);
      await pumpOverBoard(tester, client);

      expect(find.text('화면을 나가도 생성은 계속되고, 끝나면 알려 드립니다.'), findsOneWidget);

      await tester.pageBack();
      await tester.pumpAndSettle();
      expect(find.byType(CreateTaskScreen), findsNothing);
      final boardCallsBeforeCreated = client.boardCalls.length;

      pending.complete();
      await tester.pumpAndSettle();

      expect(client.boardCalls.length, greaterThan(boardCallsBeforeCreated));
      expect(find.text('feat/x 태스크를 만들었습니다.'), findsOneWidget);

      await tester.tap(find.widgetWithText(SnackBarAction, '열기'));
      await tester.pumpAndSettle();
      expect(find.byType(TaskDetailScreen), findsOneWidget);
    });

    testWidgets('실패하면 사유를 안내한다', (tester) async {
      final pending = Completer<void>();
      final client = _HeldCreateClient(
        pendingCreate: pending,
        createError: const DesktopRequestException(
          400,
          message: '이미 있는 브랜치입니다',
        ),
      );
      await pumpOverBoard(tester, client);

      await tester.pageBack();
      await tester.pumpAndSettle();
      pending.complete();
      await tester.pumpAndSettle();

      expect(find.text('feat/x 태스크를 만들지 못했습니다: 이미 있는 브랜치입니다'), findsOneWidget);
    });
  });

  testWidgets('등록된 프로젝트가 없으면 폼 대신 데스크탑에서 하라고 안내한다', (tester) async {
    await pumpCreate(tester, projects: const []);

    expect(find.byType(TextFormField), findsNothing);
    expect(find.textContaining('데스크탑에서 프로젝트를 먼저 등록해 주세요.'), findsOneWidget);
  });

  testWidgets('브랜치 목록을 받는 동안 조회 중임을 알린다', (tester) async {
    final client = await pumpCreate(tester);
    final heldBranches = Completer<List<String>>();
    client.onFetchProjectBranches = () => heldBranches.future;
    await selectProject(tester);

    expect(find.text('브랜치 목록을 불러오는 중…'), findsOneWidget);

    heldBranches.complete(const ['main', 'dev']);
    await tester.pumpAndSettle();

    expect(find.text('브랜치 목록을 불러오는 중…'), findsNothing);
  });

  testWidgets('브랜치 목록을 받지 못하면 기본 브랜치만 있는 척하지 않고 다시 불러오기를 보인다', (tester) async {
    final client = await pumpCreate(tester);
    client.onFetchProjectBranches = () async =>
        throw const DesktopRequestException(503);
    await selectProject(tester);

    expect(find.text('브랜치 목록을 불러오지 못해 기본 브랜치만 보입니다.'), findsOneWidget);

    client.onFetchProjectBranches = null;
    await tester.tap(find.text('다시 불러오기'));
    await tester.pumpAndSettle();

    expect(find.text('브랜치 목록을 불러오지 못해 기본 브랜치만 보입니다.'), findsNothing);
    await tester.tap(find.byType(DropdownButtonFormField<String>));
    await tester.pumpAndSettle();
    expect(find.text('main').last, findsOneWidget);
  });
}

/// 생성 응답을 붙잡아 두는 클라이언트. 응답 전에 화면을 떠나는 흐름을 재현한다
class _HeldCreateClient extends FakeDesktopClient {
  _HeldCreateClient({required this.pendingCreate, super.createError});

  final Completer<void> pendingCreate;

  @override
  Future<Map<String, dynamic>> createTask(
    DesktopConnection connection, {
    required String projectId,
    required String branchName,
    String? baseBranch,
    String? description,
    String? priority,
  }) async {
    await pendingCreate.future;
    return super.createTask(
      connection,
      projectId: projectId,
      branchName: branchName,
      baseBranch: baseBranch,
      description: description,
      priority: priority,
    );
  }
}
