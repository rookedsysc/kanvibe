import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/notifications/application/mobile_notification_controller.dart';
import 'package:kanvibe_mobile/src/features/notifications/application/notification_inbox_controller.dart';
import 'package:kanvibe_mobile/src/features/notifications/domain/inbox_notification.dart';

import '../../../support/fake_desktop.dart';
import '../../../support/fake_notification_inbox.dart';
import '../../../support/fake_notifications.dart';

void main() {
  const pcA = DesktopConnection(host: 'a', port: 9736, token: 'a');
  const pcB = DesktopConnection(host: 'b', port: 9736, token: 'b');
  late InboxDesktopClient client;
  late FakeMobileNotifications native;
  late ProviderContainer container;

  setUp(() async {
    client = InboxDesktopClient();
    client.notificationsByConnection[pcA.id] = [
      inboxNotice(),
      inboxNotice(id: 'read', isRead: true),
    ];
    client.notificationsByConnection[pcB.id] = [inboxNotice(id: 'b')];
    native = FakeMobileNotifications();
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
        mobileNotificationsProvider.overrideWithValue(native),
      ],
    );
    await container.read(notificationInboxProvider.future);
  });
  tearDown(() async {
    container.dispose();
    await native.dispose();
  });

  test('현재 PC의 알림과 미읽음 개수를 같은 목록에서 계산한다', () {
    final state = container.read(notificationInboxProvider).requireValue;
    expect(state.notifications.map((entry) => entry.id), ['n1', 'read']);
    expect(state.unreadCount, 1);
  });

  test('데스크톱 변경 신호는 현재 PC만 재조회하고 OS 알림을 만들지 않는다', () async {
    client.notificationsByConnection[pcA.id] = [inboxNotice(isRead: true)];
    final before = client.inboxFetches.length;
    client.openedEvents[pcB.id]!.add({'type': 'notifications-changed'});
    await Future<void>(() {});
    expect(client.inboxFetches.length, before);
    client.openedEvents[pcA.id]!.add({'type': 'notifications-changed'});
    await Future<void>(() {});
    expect(
      (await container.read(notificationInboxProvider.future)).unreadCount,
      0,
    );
    expect(client.inboxFetches.length, greaterThan(before));
    expect(native.shown, isEmpty);
  });

  test('재연결과 전경 복귀는 놓친 읽음 상태를 다시 조회한다', () async {
    client.notificationsByConnection[pcA.id] = [];
    client.openedEvents[pcA.id]!.add({'type': 'connected'});
    await Future<void>(() {});
    expect(
      (await container.read(notificationInboxProvider.future)).notifications,
      isEmpty,
    );
    final events = container.read(mobileNotificationControllerProvider);
    events.setActive(false);
    client.notificationsByConnection[pcA.id] = [inboxNotice(id: 'resumed')];
    events.setActive(true);
    await Future<void>(() {});
    expect(
      (await container.read(notificationInboxProvider.future))
          .notifications
          .single
          .id,
      'resumed',
    );
  });

  test('중복 읽음은 한 번만 보내며 응답 전에는 목록을 보존한다', () async {
    final pending = Completer<void>();
    client.onRead = () => pending.future;
    final notifier = container.read(notificationInboxProvider.notifier);
    final first = notifier.markRead('n1');
    expect(await notifier.markRead('n1'), isFalse);
    await Future<void>(() {});
    expect(client.readCalls, hasLength(1));
    expect(
      container.read(notificationInboxProvider).requireValue.unreadCount,
      1,
    );
    expect(
      container.read(notificationInboxProvider).requireValue.isUpdating,
      isTrue,
    );
    pending.complete();
    expect(await first, isTrue);
    expect(
      container.read(notificationInboxProvider).requireValue.unreadCount,
      0,
    );
  });

  test('읽음 대기 중 들어온 새 알림도 완료 뒤 목록에 남는다', () async {
    final pending = Completer<void>();
    client.onRead = () => pending.future;
    final request = container
        .read(notificationInboxProvider.notifier)
        .markRead('n1');
    await Future<void>(() {});
    client.notificationsByConnection[pcA.id]!.add(inboxNotice(id: 'new'));
    client.openedEvents[pcA.id]!.add({'type': 'notifications-changed'});
    await Future<void>(() {});
    pending.complete();
    expect(await request, isTrue);
    final state = await container.read(notificationInboxProvider.future);
    expect(
      state.notifications.map((entry) => entry.id),
      containsAll(['n1', 'new']),
    );
    expect(state.unreadCount, 1);
  });

  test('읽음 실패는 기존 읽음 상태를 유지하고 다시 시도할 수 있다', () async {
    client.onRead = () async => throw const DesktopRequestException(500);
    final notifier = container.read(notificationInboxProvider.notifier);
    await expectLater(
      notifier.markRead('n1'),
      throwsA(isA<DesktopRequestException>()),
    );
    expect(
      container.read(notificationInboxProvider).requireValue.unreadCount,
      1,
    );
    expect(
      container.read(notificationInboxProvider).requireValue.isUpdating,
      isFalse,
    );
    client.onRead = null;
    expect(await notifier.markRead('n1'), isTrue);
  });

  test('요청 직전 provider가 폐기되면 읽음 요청을 보내지 않는다', () async {
    final request = container
        .read(notificationInboxProvider.notifier)
        .markRead('n1');
    container.invalidate(notificationInboxProvider);
    await container.read(notificationInboxProvider.future);
    expect(await request, isFalse);
    expect(client.readCalls, isEmpty);
  });

  test('PC 변경 뒤 늦게 끝난 읽음은 다른 PC의 목록을 덮지 않는다', () async {
    final pending = Completer<void>();
    client.onRead = () => pending.future;
    final request = container
        .read(notificationInboxProvider.notifier)
        .markRead('n1');
    await Future<void>(() {});
    await container.read(connectionControllerProvider.notifier).select(pcB.id);
    expect(
      (await container.read(notificationInboxProvider.future))
          .notifications
          .single
          .id,
      'b',
    );
    pending.complete();
    expect(await request, isFalse);
    expect(
      container
          .read(notificationInboxProvider)
          .requireValue
          .notifications
          .single
          .id,
      'b',
    );
    expect(client.readCalls.single.connectionId, pcA.id);
  });

  test('PC 변경 뒤 늦게 끝난 조회도 다른 PC 목록을 덮지 않는다', () async {
    final pending = Completer<List<InboxNotification>>();
    client.onFetch = (connection) async =>
        connection.id == pcA.id ? pending.future : [inboxNotice(id: 'b')];
    container.invalidate(notificationInboxProvider);
    container.read(notificationInboxProvider);
    await Future<void>(() {});
    await container.read(connectionControllerProvider.notifier).select(pcB.id);
    expect(
      (await container.read(notificationInboxProvider.future))
          .notifications
          .single
          .id,
      'b',
    );
    pending.complete([inboxNotice(id: 'stale')]);
    await Future<void>(() {});
    expect(
      container
          .read(notificationInboxProvider)
          .requireValue
          .notifications
          .single
          .id,
      'b',
    );
  });

  test('알림 읽음 401은 거절된 토큰의 PC만 제거한다', () async {
    client.onRead = () async => throw const DesktopRequestException(401);
    await expectLater(
      container.read(notificationInboxProvider.notifier).markRead('n1'),
      throwsA(isA<DesktopRequestException>()),
    );
    final registry = await container.read(connectionControllerProvider.future);
    expect(registry.find(pcA.id), isNull);
    expect(registry.find(pcB.id)?.token, pcB.token);
  });

  test('전체 읽음은 서버 완료 뒤 모든 항목의 배지를 지운다', () async {
    expect(
      await container.read(notificationInboxProvider.notifier).markAllRead(),
      isTrue,
    );
    expect(
      container.read(notificationInboxProvider).requireValue.unreadCount,
      0,
    );
    expect(client.readAllCalls, [pcA.id]);
  });
  test('읽음 응답을 기다리는 중 새로고침해도 조회 중 들어온 새 알림을 반영한다', () async {
    final subscription = container.listen(notificationInboxProvider, (_, _) {});
    addTearDown(subscription.close);
    final pendingRead = Completer<void>();
    client.onRead = () async {
      client.notificationsByConnection[pcA.id] = [inboxNotice(isRead: true)];
      client.openedEvents[pcA.id]!.add({'type': 'notifications-changed'});
      await pendingRead.future;
    };
    final read = container
        .read(notificationInboxProvider.notifier)
        .markRead('n1');
    await Future<void>(() {});
    final captured = List<InboxNotification>.of(
      client.notificationsByConnection[pcA.id]!,
    );
    final pendingFetch = Completer<List<InboxNotification>>();
    final requested = Completer<void>();
    var firstRefresh = true;
    client.onFetch = (_) async {
      if (firstRefresh) {
        firstRefresh = false;
        requested.complete();
        return pendingFetch.future;
      }
      return List.of(client.notificationsByConnection[pcA.id]!);
    };
    container.invalidate(notificationInboxProvider);
    final refresh = container.read(notificationInboxProvider.future);
    await requested.future;
    expect(container.read(notificationInboxProvider).isLoading, isTrue);
    client.notificationsByConnection[pcA.id]!.add(
      inboxNotice(id: 'during-refresh'),
    );
    client.openedEvents[pcA.id]!.add({'type': 'notifications-changed'});
    await Future<void>(() {});
    pendingFetch.complete(captured);
    await refresh;
    pendingRead.complete();
    await read;
    await Future<void>(() {});
    final current = await container.read(notificationInboxProvider.future);
    expect(
      current.notifications.map((entry) => entry.id),
      contains('during-refresh'),
    );
    expect(current.unreadCount, 1);
  });

  test('태스크 읽음은 현재 PC에 요청한 뒤 데스크톱 상태를 다시 조회해 배지를 지운다', () async {
    client.notificationsByConnection[pcA.id] = [
      inboxNotice(),
      inboxNotice(id: 'n2'),
      inboxNotice(id: 'other', taskId: 'task-2'),
    ];
    container.invalidate(notificationInboxProvider);
    await container.read(notificationInboxProvider.future);

    await container
        .read(notificationInboxProvider.notifier)
        .markTaskRead('task-1');

    expect(client.readTaskCalls, [(connectionId: pcA.id, taskId: 'task-1')]);
    final state = await container.read(notificationInboxProvider.future);
    expect(state.unreadCountForTask('task-1'), 0);
    expect(state.unreadCountForTask('task-2'), 1);
  });
}
