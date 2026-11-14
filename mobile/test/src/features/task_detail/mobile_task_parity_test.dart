import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/board/domain/board_project.dart';
import 'package:kanvibe_mobile/src/features/board/domain/board_task.dart';
import 'package:kanvibe_mobile/src/features/board/domain/task_status.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/notifications/application/mobile_notification_controller.dart';
import 'package:kanvibe_mobile/src/features/task_create/domain/session_type.dart';
import 'package:kanvibe_mobile/src/features/task_create/presentation/create_task_screen.dart';
import 'package:kanvibe_mobile/src/features/task_detail/domain/mirror_pane.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/task_detail_screen.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/task_status_sheet.dart';

import '../../../support/fake_desktop.dart';
import '../../../support/fake_notifications.dart';

const task = BoardTask(
  id: 'same-task',
  title: '작업',
  status: TaskStatus.progress,
  projectId: 'project',
  branchName: 'feat/existing',
);
const pcA = DesktopConnection(host: '127.0.0.1', port: 9736, token: 'a');
const pcB = DesktopConnection(host: '127.0.0.2', port: 9736, token: 'b');

typedef JsonCall = ({
  DesktopConnection connection,
  String method,
  List<String> path,
  Map<String, dynamic>? body,
});

class ParityClient extends FakeDesktopClient {
  ParityClient() : super(surfaces: const TaskSurfaces(tabs: []));
  final requests = <JsonCall>[];
  Future<Map<String, dynamic>> Function(JsonCall)? respond;
  @override
  Future<Map<String, dynamic>> requestJson(
    DesktopConnection connection,
    String method,
    List<String> segments, {
    Map<String, String>? query,
    Map<String, dynamic>? body,
    Duration timeout = const Duration(seconds: 30),
  }) async {
    final call = (
      connection: connection,
      method: method,
      path: segments,
      body: body,
    );
    requests.add(call);
    return respond?.call(call) ??
        {
          'success': true,
          'settings': {'doneAlertDismissed': false},
        };
  }

  @override
  Future<void> markTaskNotificationsRead(
    DesktopConnection connection,
    String taskId,
  ) async {
    await requestJson(connection, 'POST', [
      'tasks',
      taskId,
      'notifications',
      'read',
    ]);
  }

  List<JsonCall> get reads => requests
      .where((c) => c.path.join('/') == 'tasks/same-task/notifications/read')
      .toList();
}

Widget testApp(
  Widget child,
  ParityClient client, {
  GlobalKey<NavigatorState>? navigatorKey,
  ProviderContainer? container,
  FakeMobileNotifications? notifications,
}) {
  final app = MaterialApp(navigatorKey: navigatorKey, home: child);
  if (container != null) {
    return UncontrolledProviderScope(container: container, child: app);
  }
  return ProviderScope(
    overrides: [
      desktopClientProvider.overrideWithValue(client),
      connectionStoreProvider.overrideWithValue(FakeConnectionStore()),
      if (notifications != null)
        mobileNotificationsProvider.overrideWithValue(notifications),
    ],
    child: app,
  );
}

Future<void> openDone(WidgetTester tester, ParityClient client) async {
  await tester.pumpWidget(
    testApp(const Scaffold(body: TaskStatusSheet(task: task)), client),
  );
  await tester.pumpAndSettle();
  await ProviderScope.containerOf(tester.element(find.byType(TaskStatusSheet)))
      .read(connectionControllerProvider.future);
  await tester.pump();
  await tester.tap(find.text('Done(으)로 옮기기'));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('Done settings read failure blocks a state transition', (
    tester,
  ) async {
    final client = ParityClient()
      ..respond = (_) async => throw DesktopRequestException(500);
    await openDone(tester, client);
    expect(client.statusCalls, isEmpty);
    expect(find.text('Done 설정을 확인하지 못했습니다. 다시 시도해 주세요.'), findsOneWidget);
  });

  testWidgets('Done dismissal persists before the status mutation is sent', (
    tester,
  ) async {
    final saved = Completer<Map<String, dynamic>>();
    final client = ParityClient()
      ..respond = (call) async => call.method == 'PATCH'
          ? saved.future
          : {
              'settings': {'doneAlertDismissed': false},
            };
    await openDone(tester, client);
    expect(find.textContaining('worktree·브랜치·세션은 유지'), findsOneWidget);
    await tester.tap(find.byType(Checkbox));
    await tester.pump();
    await tester.tap(find.text('Done으로 옮기기'));
    await tester.pump();
    expect(client.statusCalls, isEmpty);
    expect(client.requests.last.method, 'PATCH');
    expect(client.requests.last.path, ['settings']);
    expect(client.requests.last.body, {'doneAlertDismissed': true});
    saved.complete({'success': true});
    await tester.pumpAndSettle();
    expect(client.statusCalls.single.taskId, task.id);
    expect(client.statusCalls.single.status, TaskStatus.done);
  });

  testWidgets('failed Done dismissal stays open and sends no status mutation', (
    tester,
  ) async {
    final client = ParityClient()
      ..respond = (call) async {
        if (call.method == 'PATCH') throw DesktopRequestException(500);
        return {
          'settings': {'doneAlertDismissed': false},
        };
      };
    await openDone(tester, client);
    await tester.tap(find.byType(Checkbox));
    await tester.pump();
    await tester.tap(find.text('Done으로 옮기기'));
    await tester.pumpAndSettle();
    expect(
      client.requests.where(
        (call) => call.method == 'PATCH' && call.path.join('/') == 'settings',
      ),
      hasLength(1),
    );
    expect(find.text('설정을 저장하지 못했습니다. 다시 시도해 주세요.'), findsOneWidget);
    expect(client.statusCalls, isEmpty);
    expect(find.text('Done으로 옮기기'), findsOneWidget);
  });

  for (final duringSave in [false, true]) {
    testWidgets(
      'PC change while Done ${duringSave ? 'saving dismissal' : 'reading settings'} blocks the transition',
      (tester) async {
        final pending = Completer<Map<String, dynamic>>();
        final client = ParityClient()
          ..respond = (call) async {
            if (duringSave && call.method == 'GET') {
              return {
                'settings': {'doneAlertDismissed': false},
              };
            }
            return pending.future;
          };
        final container = ProviderContainer(
          overrides: [
            desktopClientProvider.overrideWithValue(client),
            connectionStoreProvider.overrideWithValue(
              FakeConnectionStore(
                initial: const SavedConnections(
                  connections: [pcA, pcB],
                  activeId: '127.0.0.1:9736',
                ),
              ),
            ),
          ],
        );
        addTearDown(container.dispose);
        await container.read(connectionControllerProvider.future);
        await tester.pumpWidget(
          testApp(
            const Scaffold(body: TaskStatusSheet(task: task)),
            client,
            container: container,
          ),
        );
        await tester.pumpAndSettle();
        await tester.tap(find.text('Done(으)로 옮기기'));
        await tester.pumpAndSettle();
        if (duringSave) {
          await tester.tap(find.byType(Checkbox));
          await tester.pump();
          await tester.tap(find.text('Done으로 옮기기'));
          await tester.pump();
          expect(client.requests.last.connection, pcA);
          expect(client.requests.last.method, 'PATCH');
        }
        await container
            .read(connectionControllerProvider.notifier)
            .select(pcB.id);
        pending.complete(
          duringSave
              ? {'success': true}
              : {
                  'settings': {'doneAlertDismissed': true},
                },
        );
        await tester.pumpAndSettle();
        expect(client.statusCalls, isEmpty);
        expect(client.requests.every((call) => call.connection == pcA), isTrue);
        expect(find.text('Done으로 옮길까요?'), findsNothing);
      },
    );
  }

  testWidgets('stored Done dismissal skips confirmation', (tester) async {
    final client = ParityClient()
      ..respond = (_) async => {
        'settings': {'doneAlertDismissed': true},
      };
    await openDone(tester, client);
    expect(find.text('Done으로 옮길까요?'), findsNothing);
    expect(client.statusCalls.single.status, TaskStatus.done);
  });

  testWidgets(
    'branch conversion preserves the existing task ID and explicit session type',
    (tester) async {
      final navigator = GlobalKey<NavigatorState>();
      final client = ParityClient()
        ..respond = (_) async => {
          'task': {
            'id': 'same-task',
            'title': 'feat/new',
            'status': 'progress',
            'projectId': 'project',
            'branchName': 'feat/new',
          },
        };
      BoardTask? result;
      await tester.pumpWidget(
        testApp(
          const Scaffold(body: Text('home')),
          client,
          navigatorKey: navigator,
        ),
      );
      await tester.pumpAndSettle();
      final route = navigator.currentState!.push<BoardTask>(
        MaterialPageRoute(
          builder: (_) => const CreateTaskScreen(
            projects: [
              BoardProject(
                id: 'project',
                name: 'repo',
                defaultBranch: 'main',
                isWorktree: false,
              ),
            ],
            defaultProjectId: 'project',
            existingTaskId: 'same-task',
          ),
        ),
      );
      route.then((value) => result = value);
      await tester.pumpAndSettle();
      await tester.enterText(find.byType(TextFormField).first, 'feat/new');
      await tester.ensureVisible(
        find.byType(DropdownButtonFormField<SessionType>),
      );
      await tester.tap(find.byType(DropdownButtonFormField<SessionType>));
      await tester.pumpAndSettle();
      await tester.tap(find.text('tmux').last);
      await tester.pumpAndSettle();
      await tester.ensureVisible(find.widgetWithText(FilledButton, '만들기'));
      await tester.tap(find.widgetWithText(FilledButton, '만들기'));
      await tester.pumpAndSettle();
      expect(client.createCalls, isEmpty);
      final call = client.requests.single;
      expect(call.method, 'POST');
      expect(call.path, ['tasks', 'same-task', 'branch']);
      expect(call.body, {
        'projectId': 'project',
        'branchName': 'feat/new',
        'baseBranch': 'main',
        'sessionType': 'tmux',
      });
      expect(result?.id, 'same-task');
      expect(result?.branchName, 'feat/new');
    },
  );

  testWidgets(
    'visible task reads on entry and new events, covered route waits until return',
    (tester) async {
      final navigator = GlobalKey<NavigatorState>();
      final notifications = FakeMobileNotifications();
      final client = ParityClient();
      await tester.pumpWidget(
        testApp(
          const TaskDetailScreen(task: task),
          client,
          navigatorKey: navigator,
          notifications: notifications,
        ),
      );
      await tester.pumpAndSettle();
      expect(client.reads, isNotEmpty);
      var previous = client.reads.length;
      client.openedEvents['127.0.0.1:9736']!.add(
        taskNotification(taskId: task.id, eventId: 'visible'),
      );
      await tester.pumpAndSettle();
      expect(client.reads.length, greaterThan(previous));
      unawaited(
        navigator.currentState!.push<void>(
          MaterialPageRoute(
            builder: (_) => const Scaffold(body: Text('cover')),
          ),
        ),
      );
      await tester.pumpAndSettle();
      previous = client.reads.length;
      client.openedEvents['127.0.0.1:9736']!.add(
        taskNotification(taskId: task.id, eventId: 'covered'),
      );
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
      await tester.pumpAndSettle();
      expect(client.reads.length, previous);
      navigator.currentState!.pop();
      await tester.pumpAndSettle();
      expect(client.reads.length, greaterThan(previous));
      await tester.pumpWidget(const SizedBox.shrink());
      await notifications.dispose();
    },
  );

  testWidgets(
    'a delayed read 401 for PC A never removes the newly selected PC B',
    (tester) async {
      final response = Completer<Map<String, dynamic>>();
      final client = ParityClient()
        ..respond = (call) async =>
            call.path.last == 'read' ? response.future : {'success': true};
      final container = ProviderContainer(
        overrides: [
          desktopClientProvider.overrideWithValue(client),
          connectionStoreProvider.overrideWithValue(
            FakeConnectionStore(
              initial: const SavedConnections(
                connections: [pcA, pcB],
                activeId: '127.0.0.1:9736',
              ),
            ),
          ),
        ],
      );
      addTearDown(container.dispose);
      await tester.pumpWidget(
        testApp(
          const TaskDetailScreen(task: task),
          client,
          container: container,
        ),
      );
      await tester.pumpAndSettle();
      expect(client.reads.single.connection, pcA);
      await container
          .read(connectionControllerProvider.notifier)
          .select(pcB.id);
      await tester.pumpAndSettle();
      response.completeError(DesktopRequestException(401));
      await tester.pumpAndSettle();
      final registry = await container.read(
        connectionControllerProvider.future,
      );
      expect(registry.find(pcB.id), pcB);
      expect(registry.activeConnection, pcB);
    },
  );
}
