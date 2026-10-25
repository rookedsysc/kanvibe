import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/notifications/application/mobile_notification_controller.dart';

import '../../../support/fake_desktop.dart';
import '../../../support/fake_notifications.dart';

Future<void> flushEvents() => Future<void>(() {});

void main() {
  const pcA = DesktopConnection(host: '127.0.0.1', port: 9736, token: 'a');
  const pcB = DesktopConnection(host: '127.0.0.2', port: 9736, token: 'b');

  late FakeDesktopClient client;
  late FakeMobileNotifications notifications;
  late ProviderContainer container;

  setUp(() async {
    client = FakeDesktopClient();
    notifications = FakeMobileNotifications();
    container = ProviderContainer(
      overrides: [
        desktopClientProvider.overrideWithValue(client),
        connectionStoreProvider.overrideWithValue(
          FakeConnectionStore(
            initial: SavedConnections(
              connections: [pcA, pcB],
              activeId: pcA.id,
            ),
          ),
        ),
        mobileNotificationsProvider.overrideWithValue(notifications),
      ],
    );
    container.read(mobileNotificationControllerProvider);
    await container.read(connectionControllerProvider.future);
    await flushEvents();
  });

  tearDown(() async {
    container.dispose();
    await notifications.dispose();
  });

  test('저장한 모든 PC를 구독하고 전경 푸시와 소켓의 동일 이벤트는 한 번만 표시한다', () async {
    expect(client.openedEvents.keys, containsAll([pcA.id, pcB.id]));
    client.openedEvents[pcA.id]!.add(taskNotification());
    notifications.foreground.add(taskNotification());
    await flushEvents();
    expect(notifications.shown, hasLength(1));
    expect(notifications.shown.single['connectionId'], pcA.id);
  });

  test('다른 PC가 같은 eventId를 보내도 서로 다른 알림으로 표시한다', () async {
    client.openedEvents[pcA.id]!.add(taskNotification());
    client.openedEvents[pcB.id]!.add(taskNotification());
    await flushEvents();
    expect(notifications.shown, hasLength(2));
    expect(notifications.shown.last['connectionId'], pcB.id);
  });

  test('백그라운드에서는 이벤트 소켓을 닫고 복귀할 때 다시 구독한다', () async {
    final controller = container.read(mobileNotificationControllerProvider);
    controller.setActive(false);
    await flushEvents();
    expect(client.closedEvents, containsAll([pcA.id, pcB.id]));
    expect(notifications.shown, isEmpty);
    controller.setActive(true);
    await flushEvents();
    expect(client.openedEvents[pcA.id]!.isClosed, isFalse);
    expect(client.openedEvents[pcB.id]!.isClosed, isFalse);
  });

  test('토큰 갱신을 모든 PC에 등록하고 연결된 PC의 식별자를 보낸다', () async {
    notifications.tokens.add('fresh-push-token');
    await flushEvents();
    expect(client.notificationRegistrations, hasLength(2));
    expect(
      client.notificationRegistrations.map((entry) => entry['connectionId']),
      containsAll([pcA.id, pcB.id]),
    );
    expect(
      client.notificationRegistrations.every(
        (entry) => entry['pushToken'] == 'fresh-push-token',
      ),
      isTrue,
    );
  });

  test('기기 연결 해제 종료 코드는 해당 PC만 제거하고 다시 연결하지 않는다', () async {
    client.eventCloseCodes[pcB.id] = 4001;
    await client.openedEvents[pcB.id]!.close();
    await flushEvents();
    final registry = await container.read(connectionControllerProvider.future);
    expect(registry.find(pcB.id), isNull);
    expect(registry.find(pcA.id), isNotNull);
  });

  test('연결되지 않은 PC나 잘못된 이벤트는 시스템 알림으로 표시하지 않는다', () async {
    notifications.foreground.add(
      taskNotification(connectionId: 'unknown:9736'),
    );
    client.openedEvents[pcA.id]!.add({
      'type': 'task-status-changed',
      'taskId': 'a',
    });
    await flushEvents();
    expect(notifications.shown, isEmpty);
  });

  test('board-updated와 connected는 현재 보드를 다시 조회한다', () async {
    final observed = <Object?>[];
    final subscription = container.listen(
      boardProvider,
      (_, next) => observed.add(next),
      fireImmediately: true,
    );
    addTearDown(subscription.close);
    await container.read(boardProvider.future);
    observed.clear();
    client.openedEvents[pcA.id]!.add({'type': 'board-updated'});
    await flushEvents();
    await container.read(boardProvider.future);
    expect(observed, isNotEmpty);
    observed.clear();
    client.openedEvents[pcA.id]!.add({'type': 'connected'});
    await flushEvents();
    await container.read(boardProvider.future);
    expect(observed, isNotEmpty);
  });
}
