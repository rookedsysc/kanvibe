import 'dart:async';
import 'dart:convert';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/project_registry/application/project_registry_controller.dart';
import 'package:kanvibe_mobile/src/features/project_registry/presentation/project_registry_screen.dart';
import 'package:kanvibe_mobile/src/features/project_registry/presentation/registered_projects_screen.dart';

import '../ai_accounts/ai_test_support.dart';

Map<String, dynamic> scanResult() => {
  'success': true,
  'result': {
    'registered': ['Remote project'],
    'skipped': [],
    'errors': [],
    'worktreeTasks': [],
    'registeredWorktrees': [],
  },
};
void main() {
  test('호스트 전환은 경로를 초기화하고 이전 SSH 응답을 폐기한다', () async {
    final remote = Completer<http.Response>();
    final client = AiStreamClient(
      (request) async => request.url.queryParameters['sshHost'] == 'work'
          ? remote.future
          : aiResponse({
              'success': true,
              'directories': ['Local folder'],
            }),
    );
    final container = ProviderContainer(
      overrides: [
        desktopClientProvider.overrideWithValue(client),
        connectionStoreProvider.overrideWithValue(AiMemoryConnections()),
      ],
    );
    addTearDown(() {
      container.dispose();
      client.close();
    });
    final provider = projectRegistryProvider(fixturePc);
    container.listen(provider, (_, _) {});
    final controller = container.read(provider.notifier);
    await controller.browse('/local');
    final pending = controller.selectHost('work');
    await Future<void>.delayed(Duration.zero);
    await controller.selectHost(null);
    remote.complete(
      aiResponse({
        'success': true,
        'directories': ['Old remote'],
      }),
    );
    await pending;
    expect(container.read(provider).path, '~');
    expect(container.read(provider).sshHost, isNull);
    expect(container.read(provider).directories, ['Local folder']);
  });
  testWidgets('SSH 선택 후 원격 폴더를 등록하고 실패는 재시도할 수 있다', (tester) async {
    final scans = <Map<String, dynamic>>[];
    var fail = true;
    final client = AiStreamClient((request) async {
      if (request.url.path.endsWith('/hosts')) {
        return aiResponse({
          'success': true,
          'hosts': ['work'],
        });
      }
      if (request.method == 'POST') {
        scans.add(jsonDecode(request.body) as Map<String, dynamic>);
        return aiResponse(scanResult());
      }
      if (request.url.queryParameters['sshHost'] == 'work' && fail) {
        return aiResponse({
          'success': false,
          'error': 'SSH offline',
        }, status: 503);
      }
      return aiResponse({'success': true, 'directories': []});
    });
    await mountAi(
      tester,
      const ProjectRegistryScreen(connection: fixturePc),
      client,
    );
    await tester.tap(find.text('이 PC의 로컬 폴더'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 200));
    await tester.tap(find.text('work').last);
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    expect(find.textContaining('PC에 요청하지 못했습니다'), findsOneWidget);
    fail = false;
    await tester.tap(find.text('다시 시도'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    await tester.tap(find.text('이 폴더에서 프로젝트 등록'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    expect(scans, [
      {'rootPath': '~', 'sshHost': 'work'},
    ]);
    expect(find.text('Remote project'), findsOneWidget);
  });
  testWidgets('등록 삭제는 Git 보존과 태스크 등록 영향 안내 후에만 실행한다', (tester) async {
    var deleted = false;
    final client = AiStreamClient((request) async {
      if (request.method == 'DELETE') {
        deleted = true;
        return aiResponse({'success': true});
      }
      return aiResponse({
        'success': true,
        'board': {
          'projects': [
            {
              'id': 'p1',
              'name': 'Project',
              'repoPath': '/fixture/repo',
              'defaultBranch': 'main',
              'isWorktree': false,
            },
          ],
          'tasks': {},
        },
      });
    });
    await mountAi(
      tester,
      const RegisteredProjectsScreen(connection: fixturePc),
      client,
    );
    await tester.tap(find.byTooltip('프로젝트 등록 삭제'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 200));
    expect(find.textContaining('Git 저장소와 폴더는 보존'), findsOneWidget);
    expect(deleted, isFalse);
    await tester.tap(find.text('삭제'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 200));
    expect(deleted, isTrue);
    expect(find.text('등록된 프로젝트가 없습니다.'), findsOneWidget);
  });
}
