import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/board/domain/board_task.dart';
import 'package:kanvibe_mobile/src/features/board/domain/task_status.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/task_editor_screen.dart';

import '../../../support/fake_desktop.dart';

const _pcA = DesktopConnection(host: 'pc-a', port: 4885, token: 'token-a');
const _pcB = DesktopConnection(host: 'pc-b', port: 4885, token: 'token-b');
const _task = BoardTask(
  id: 'shared-task',
  title: '작업',
  status: TaskStatus.progress,
  projectId: 'shared-project',
  description: '기존 설명',
  projectColor: '#0064FF',
);

class _Client extends FakeDesktopClient {
  final writes =
      <
        ({
          DesktopConnection connection,
          List<String> path,
          Map<String, dynamic>? body,
        })
      >[];
  final taskJson = <String, dynamic>{
    'id': 'shared-task',
    'title': '작업',
    'status': 'progress',
    'description': '기존 설명',
    'projectId': 'shared-project',
    'project': {'id': 'shared-project', 'name': '프로젝트', 'color': '#0064FF'},
  };
  @override
  Future<Map<String, dynamic>?> fetchTask(
    DesktopConnection connection,
    String taskId,
  ) async => Map.of(taskJson);
  @override
  Future<Map<String, dynamic>> requestJson(
    DesktopConnection connection,
    String method,
    List<String> segments, {
    Map<String, String>? query,
    Map<String, dynamic>? body,
    Duration timeout = const Duration(seconds: 30),
  }) async {
    if (method == 'PATCH') {
      writes.add((connection: connection, path: segments, body: body));
      if (segments.last == 'color') {
        taskJson['project'] = {
          'id': 'shared-project',
          'name': '프로젝트',
          'color': body!['color'],
        };
      } else {
        taskJson.addAll(body!);
      }
    }
    return {'success': true, 'task': Map.of(taskJson)};
  }
}

ProviderContainer _container(_Client client) => ProviderContainer(
  overrides: [
    desktopClientProvider.overrideWithValue(client),
    connectionStoreProvider.overrideWithValue(
      FakeConnectionStore(
        initial: const SavedConnections(
          connections: [_pcA, _pcB],
          activeId: 'pc-a:4885',
        ),
      ),
    ),
  ],
);

Future<void> _mount(
  WidgetTester tester,
  ProviderContainer container,
  GlobalKey<NavigatorState> navigator,
) async {
  await container.read(connectionControllerProvider.future);
  await tester.pumpWidget(
    UncontrolledProviderScope(
      container: container,
      child: MaterialApp(
        navigatorKey: navigator,
        home: const Scaffold(body: Text('보드')),
      ),
    ),
  );
  await tester.pumpAndSettle();
  navigator.currentState!.push<void>(
    MaterialPageRoute(builder: (_) => const TaskEditorScreen(task: _task)),
  );
  await tester.pumpAndSettle();
}

Future<void> _openColor(WidgetTester tester) async {
  await tester.ensureVisible(find.widgetWithText(TextButton, '프로젝트 색상'));
  await tester.tap(find.widgetWithText(TextButton, '프로젝트 색상'));
  await tester.pumpAndSettle();
  await tester.enterText(
    find.descendant(
      of: find.byType(AlertDialog),
      matching: find.byType(TextFormField),
    ),
    '#112233',
  );
}

Future<void> _confirmColor(WidgetTester tester) async {
  await tester.tap(
    find.descendant(
      of: find.byType(AlertDialog),
      matching: find.widgetWithText(TextButton, '저장'),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  testWidgets(
    'same PC saves description and color; changing PC blocks both old drafts',
    (tester) async {
      final client = _Client();
      final container = _container(client);
      addTearDown(container.dispose);
      final navigator = GlobalKey<NavigatorState>();
      await _mount(tester, container, navigator);
      await _openColor(tester);
      await _confirmColor(tester);
      expect(client.writes.single.connection, _pcA);
      expect(client.writes.single.path, [
        'projects',
        'shared-project',
        'color',
      ]);
      expect(client.writes.single.body, {'color': '#112233'});
      expect(
        tester.widget<Icon>(find.byIcon(Icons.palette_outlined)).color,
        const Color(0xff112233),
      );
      await tester.enterText(find.byType(TextFormField).first, '저장할 설명');
      await tester.ensureVisible(find.widgetWithText(FilledButton, '저장'));
      await tester.tap(find.widgetWithText(FilledButton, '저장'));
      await tester.pumpAndSettle();
      expect(client.writes.last.connection, _pcA);
      expect(client.writes.last.path, ['tasks', 'shared-task', 'details']);
      expect(client.writes.last.body, {
        'description': '저장할 설명',
        'priority': null,
      });
      expect(find.byType(TaskEditorScreen), findsNothing);
      navigator.currentState!.push<void>(
        MaterialPageRoute(builder: (_) => const TaskEditorScreen(task: _task)),
      );
      await tester.pumpAndSettle();
      await tester.enterText(find.byType(TextFormField).first, 'A에서 입력한 초안');
      await container
          .read(connectionControllerProvider.notifier)
          .select(_pcB.id);
      await tester.pumpAndSettle();
      final before = client.writes.length;
      await tester.ensureVisible(find.widgetWithText(FilledButton, '저장'));
      await tester.tap(find.widgetWithText(FilledButton, '저장'));
      await tester.pumpAndSettle();
      await tester.ensureVisible(find.widgetWithText(TextButton, '프로젝트 색상'));
      await tester.tap(find.widgetWithText(TextButton, '프로젝트 색상'));
      await tester.pumpAndSettle();
      expect(client.writes.length, before);
      expect(find.byType(AlertDialog), findsNothing);
      expect(find.text('PC 연결이 변경되었습니다. 화면을 닫고 다시 열어 주세요.'), findsOneWidget);
    },
  );

  testWidgets('PC change while the color dialog is open sends no mutation', (
    tester,
  ) async {
    final client = _Client();
    final container = _container(client);
    addTearDown(container.dispose);
    await _mount(tester, container, GlobalKey<NavigatorState>());
    await _openColor(tester);
    await container.read(connectionControllerProvider.notifier).select(_pcB.id);
    await tester.pumpAndSettle();
    await _confirmColor(tester);
    expect(client.writes, isEmpty);
    expect(find.text('PC 연결이 변경되었습니다. 화면을 닫고 다시 열어 주세요.'), findsOneWidget);
    expect(
      tester.widget<Icon>(find.byIcon(Icons.palette_outlined)).color,
      const Color(0xff0064ff),
    );
  });
}
