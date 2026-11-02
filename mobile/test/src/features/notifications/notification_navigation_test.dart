import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/app.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/notifications/application/mobile_notification_controller.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/task_detail_screen.dart';

import '../../../support/fake_desktop.dart';
import '../../../support/fake_notifications.dart';

const pcA = DesktopConnection(
  host: '127.0.0.1',
  port: 9736,
  token: 'token-a',
  name: '작업실',
);
const pcB = DesktopConnection(
  host: '127.0.0.2',
  port: 19736,
  token: 'token-b',
  name: '집 PC',
);

class NotificationDesktopClient extends FakeDesktopClient {
  final taskLookups = <({String connectionId, String taskId})>[];
  Map<String, dynamic>? task = const {
    'id': 'outside-page',
    'title': '알림으로 연 태스크',
    'status': 'review',
    'projectId': 'project-b',
    'projectName': '알림 프로젝트',
  };
  Object? taskError;
  Completer<Map<String, dynamic>?>? pendingTask;

  @override
  Future<Map<String, dynamic>?> fetchTask(
    DesktopConnection connection,
    String taskId,
  ) async {
    taskLookups.add((connectionId: connection.id, taskId: taskId));
    if (taskError != null) throw taskError!;
    return pendingTask == null
        ? (task == null ? null : {...task!, 'id': taskId})
        : await pendingTask!.future;
  }
}

class DelayedRegistrationClient extends NotificationDesktopClient {
  final firstRegistration = Completer<void>();
  String? acceptedToken;

  @override
  Future<void> registerNotifications(
    DesktopConnection connection, {
    required String pushToken,
    required String platform,
  }) async {
    if (pushToken == 'old-token') await firstRegistration.future;
    acceptedToken = pushToken;
    await super.registerNotifications(
      connection,
      pushToken: pushToken,
      platform: platform,
    );
  }
}

Future<FakeConnectionStore> pumpNotifications(
  WidgetTester tester, {
  required NotificationDesktopClient client,
  required FakeMobileNotifications notifications,
  SavedConnections? connections,
}) async {
  final store = FakeConnectionStore(
    initial:
        connections ??
        SavedConnections(connections: [pcA, pcB], activeId: pcA.id),
  );
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        desktopClientProvider.overrideWithValue(client),
        connectionStoreProvider.overrideWithValue(store),
        mobileNotificationsProvider.overrideWithValue(notifications),
      ],
      child: const KanvibeMobileApp(),
    ),
  );
  await tester.pumpAndSettle();
  return store;
}

void main() {
  testWidgets('토큰 교체 중 오래 걸린 등록이 최신 푸시 토큰을 덮어쓰지 않는다', (tester) async {
    final notifications = FakeMobileNotifications();
    addTearDown(notifications.dispose);
    final client = DelayedRegistrationClient();
    await pumpNotifications(
      tester,
      client: client,
      notifications: notifications,
      connections: SavedConnections(connections: [pcA], activeId: pcA.id),
    );
    notifications.tokens.add('old-token');
    await tester.pump();
    notifications.tokens.add('new-token');
    await tester.pump();
    client.firstRegistration.complete();
    await tester.pumpAndSettle();
    expect(client.acceptedToken, 'new-token');
  });

  testWidgets('전경 이벤트 연결이 끊기면 재연결하고 새 이벤트를 수신한다', (tester) async {
    final notifications = FakeMobileNotifications();
    addTearDown(notifications.dispose);
    final client = NotificationDesktopClient();
    await pumpNotifications(
      tester,
      client: client,
      notifications: notifications,
      connections: SavedConnections(connections: [pcA], activeId: pcA.id),
    );
    final previous = client.openedEvents[pcA.id]!;
    await previous.close();
    await tester.pump();
    await tester.pump(const Duration(seconds: 1));
    expect(identical(client.openedEvents[pcA.id], previous), isFalse);
    client.openedEvents[pcA.id]!.add(taskNotification());
    await tester.pumpAndSettle();
    expect(notifications.shown, hasLength(1));
  });

  testWidgets('다른 PC의 알림 클릭은 해당 PC의 단건 태스크를 조회해 상세로 이동한다', (tester) async {
    final notifications = FakeMobileNotifications();
    addTearDown(notifications.dispose);
    final client = NotificationDesktopClient();
    final store = await pumpNotifications(
      tester,
      client: client,
      notifications: notifications,
    );

    notifications.opened.add(
      taskNotification(connectionId: pcB.id, taskId: 'outside-page'),
    );
    await tester.pumpAndSettle();

    expect(find.byType(TaskDetailScreen), findsOneWidget);
    expect(find.text('알림으로 연 태스크'), findsWidgets);
    expect(find.text('알림 프로젝트'), findsWidgets);
    expect(client.taskLookups, [
      (connectionId: pcB.id, taskId: 'outside-page'),
    ]);
    expect((await store.readRegistry()).activeId, pcB.id);
    expect((await store.readRegistry()).connections, hasLength(2));
  });

  testWidgets('처음 실행한 앱도 저장된 연결을 읽은 뒤 초기 알림의 태스크를 연다', (tester) async {
    final notifications = FakeMobileNotifications()
      ..initialNotification = taskNotification(
        connectionId: pcB.id,
        taskId: 'outside-page',
      );
    addTearDown(notifications.dispose);
    final store = await pumpNotifications(
      tester,
      client: NotificationDesktopClient(),
      notifications: notifications,
    );

    expect(find.byType(TaskDetailScreen), findsOneWidget);
    expect((await store.readRegistry()).activeId, pcB.id);
  });

  testWidgets('삭제된 태스크 알림은 안내를 표시하고 PC 선택을 바꾸지 않는다', (tester) async {
    final notifications = FakeMobileNotifications();
    addTearDown(notifications.dispose);
    final client = NotificationDesktopClient()..task = null;
    final store = await pumpNotifications(
      tester,
      client: client,
      notifications: notifications,
    );
    notifications.opened.add(taskNotification(connectionId: pcB.id));
    await tester.pumpAndSettle();

    expect(find.byType(TaskDetailScreen), findsNothing);
    expect(find.text('태스크가 삭제되었거나 더 이상 존재하지 않습니다.'), findsOneWidget);
    expect((await store.readRegistry()).activeId, pcA.id);
  });

  testWidgets('연결 해제된 PC의 알림은 네트워크 요청 없이 안내한다', (tester) async {
    final notifications = FakeMobileNotifications();
    addTearDown(notifications.dispose);
    final client = NotificationDesktopClient();
    await pumpNotifications(
      tester,
      client: client,
      notifications: notifications,
      connections: SavedConnections(connections: [pcA], activeId: pcA.id),
    );
    notifications.opened.add(taskNotification(connectionId: pcB.id));
    await tester.pumpAndSettle();

    expect(client.taskLookups, isEmpty);
    expect(find.text('이 알림의 PC 연결이 해제되었습니다.'), findsOneWidget);
    expect(find.byType(TaskDetailScreen), findsNothing);
  });

  testWidgets('반복 클릭해도 상세를 쌓지 않고 뒤로 가면 보드로 돌아온다', (tester) async {
    final notifications = FakeMobileNotifications();
    addTearDown(notifications.dispose);
    await pumpNotifications(
      tester,
      client: NotificationDesktopClient(),
      notifications: notifications,
    );
    notifications.opened.add(taskNotification());
    await tester.pumpAndSettle();
    notifications.opened.add(taskNotification());
    await tester.pumpAndSettle();
    await tester.pageBack();
    await tester.pumpAndSettle();

    expect(find.byType(TaskDetailScreen), findsNothing);
    expect(find.text('작업실'), findsOneWidget);
  });

  testWidgets('늦게 끝난 태스크 조회는 사용자가 바꾼 PC 선택을 되돌리지 않는다', (tester) async {
    final notifications = FakeMobileNotifications();
    addTearDown(notifications.dispose);
    final client = NotificationDesktopClient()..pendingTask = Completer();
    final store = await pumpNotifications(
      tester,
      client: client,
      notifications: notifications,
    );
    notifications.opened.add(taskNotification(connectionId: pcB.id));
    await tester.pump();
    final container = ProviderScope.containerOf(
      tester.element(find.byType(KanvibeMobileApp)),
    );
    await container.read(connectionControllerProvider.notifier).select(pcB.id);
    client.pendingTask!.complete(client.task);
    await tester.pumpAndSettle();

    expect(find.byType(TaskDetailScreen), findsNothing);
    expect((await store.readRegistry()).activeId, pcB.id);
  });
}
