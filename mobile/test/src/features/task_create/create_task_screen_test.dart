import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/features/board/domain/board_project.dart';
import 'package:kanvibe_mobile/src/features/task_create/presentation/create_task_screen.dart';

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
}) async {
  await tester.binding.setSurfaceSize(phoneSize);
  final client = FakeDesktopClient(createError: createError);

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

Future<void> selectProject(WidgetTester tester) async {
  await tester.tap(find.byType(DropdownButtonFormField<BoardProject>));
  await tester.pumpAndSettle();
  await tester.tap(find.text('kanvibe').last);
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('데스크탑 생성 창과 같은 칸을 같은 순서로 묻는다', (tester) async {
    await pumpCreate(tester);

    for (final label in ['프로젝트 *', '베이스 브랜치', '브랜치 이름 *', '설명', '우선순위', '세션 타입']) {
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
      'sessionType': 'tmux',
    });
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

    expect(find.text('태스크를 만들지 못했습니다. 데스크탑에서 자세한 사유를 확인해 주세요.'), findsOneWidget);
  });

  testWidgets('등록된 프로젝트가 없으면 폼 대신 데스크탑에서 하라고 안내한다', (tester) async {
    await pumpCreate(tester, projects: const []);

    expect(find.byType(TextFormField), findsNothing);
    expect(find.textContaining('데스크탑에서 프로젝트를 먼저 등록해 주세요.'), findsOneWidget);
  });
}
