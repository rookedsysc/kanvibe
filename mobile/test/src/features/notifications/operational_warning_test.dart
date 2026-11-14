import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/notifications/application/mobile_notification_controller.dart';
import 'package:kanvibe_mobile/src/features/notifications/domain/operational_warning.dart';

import '../../../support/fake_desktop.dart';
import '../../../support/fake_notifications.dart';

Map<String, dynamic> warning({
  String kind = 'hook-status-target-missing',
  String id = 'warning-1',
  bool notify = true,
  String connectionId = '127.0.0.1:9736',
}) => {
  'type': 'operational-warning',
  'eventId': id,
  'connectionId': connectionId,
  'taskId': 'same-task',
  'kind': kind,
  'title': '운영 경고',
  'body': '데스크탑에서 설정을 확인해 주세요.',
  'status': 'review',
  'createdAt': '2026-11-13T04:00:00.000Z',
  'shouldNotify': notify,
};
Future<void> flush() => Future<void>(() {});

void main() {
  late FakeDesktopClient client;
  late FakeMobileNotifications notifications;
  late ProviderContainer container;
  late MobileNotificationController controller;
  final warnings = <OperationalWarning>[];
  final taskUpdates = <Object>[];
  setUp(() async {
    warnings.clear();
    taskUpdates.clear();
    client = FakeDesktopClient();
    notifications = FakeMobileNotifications();
    container = ProviderContainer(
      overrides: [
        desktopClientProvider.overrideWithValue(client),
        connectionStoreProvider.overrideWithValue(FakeConnectionStore()),
        mobileNotificationsProvider.overrideWithValue(notifications),
      ],
    );
    controller = container.read(mobileNotificationControllerProvider);
    await container.read(connectionControllerProvider.future);
    await flush();
    final warningSubscription = controller.warningUpdates.listen(warnings.add);
    final taskSubscription = controller.taskUpdates.listen(taskUpdates.add);
    addTearDown(warningSubscription.cancel);
    addTearDown(taskSubscription.cancel);
  });
  tearDown(() async {
    container.dispose();
    await notifications.dispose();
  });

  test('warning socket and push dedupe without producing task status or board mutations', () async {
    final observedBoard = container.listen(boardProvider, (_, _) {});
    addTearDown(observedBoard.close);
    await container.read(boardProvider.future);
    final boardCount = client.boardCalls.length;
    client.openedEvents['127.0.0.1:9736']!.add(warning());
    notifications.foreground.add(warning());
    await flush();
    await Future<void>.delayed(const Duration(milliseconds: 350));
    expect(warnings, hasLength(1));
    expect(notifications.shown, hasLength(1));
    expect(notifications.shown.single['type'], 'operational-warning');
    expect(taskUpdates, isEmpty);
    expect(client.statusCalls, isEmpty);
    expect(client.boardCalls.length, boardCount);
  });

  test('hook installation failure remains a foreground warning when push is disabled', () async {
    client.openedEvents['127.0.0.1:9736']!.add(
      warning(kind: 'task-hook-install-failed', notify: false),
    );
    await flush();
    expect(warnings.single.kind, 'task-hook-install-failed');
    expect(notifications.shown, isEmpty);
    expect(taskUpdates, isEmpty);
  });

  test(
    'disabled target warning and unknown or unpaired events are ignored',
    () async {
      notifications.foreground.add(warning(notify: false));
      notifications.foreground.add(
        warning(id: 'unknown-kind', kind: 'arbitrary-command'),
      );
      notifications.foreground.add(
        warning(id: 'unpaired', connectionId: 'unknown:4885'),
      );
      client.openedEvents['127.0.0.1:9736']!.add({
        'type': 'operational-warning',
        'kind': 'hook-status-target-missing',
      });
      await flush();
      expect(warnings, isEmpty);
      expect(notifications.shown, isEmpty);
      expect(taskUpdates, isEmpty);
    },
  );

  test(
    'opening a warning emits a warning and never a task navigation request',
    () async {
      final opens = <Object>[];
      final subscription = controller.openRequests.listen(opens.add);
      addTearDown(subscription.cancel);
      notifications.opened.add(warning());
      await flush();
      expect(warnings, hasLength(1));
      expect(opens, isEmpty);
      expect(taskUpdates, isEmpty);
      expect(notifications.shown, isEmpty);
    },
  );

  test('initial warning is consumed as a warning and never coerced into a task status', () async {
    notifications.initialNotification = warning(id: 'initial');
    expect(controller.takeInitialNotification(), isNull);
    await flush();
    expect(warnings.single.eventId, 'initial');
    expect(taskUpdates, isEmpty);
  });

  test('warning parser rejects missing required strings and unknown kinds', () {
    expect(OperationalWarning.fromJson({...warning(), 'title': ''}), isNull);
    expect(OperationalWarning.fromJson({...warning(), 'eventId': 12}), isNull);
    expect(OperationalWarning.fromJson(warning(kind: 'invalid')), isNull);
  });
}
