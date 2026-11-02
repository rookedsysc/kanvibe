import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/features/notifications/data/mobile_notifications.dart';

const _notification = {
  'type': 'task-status-changed',
  'eventId': 'event-1',
  'taskId': 'task-1',
  'title': 'feat/push',
  'body': '입력이 필요합니다.',
  'status': 'review',
  'createdAt': '2026-10-23T00:00:00.000Z',
  'connectionId': 'desktop:9736',
};

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('dexterous.com/flutter/local_notifications');
  final calls = <MethodCall>[];
  Map<String, dynamic>? initial;

  setUp(() {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    AndroidFlutterLocalNotificationsPlugin.registerWith();
    calls.clear();
    initial = null;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          calls.add(call);
          return switch (call.method) {
            'initialize' || 'requestNotificationsPermission' => true,
            'getNotificationAppLaunchDetails' => {
              'notificationLaunchedApp': initial != null,
              if (initial != null)
                'notificationResponse': {
                  'notificationId': 1,
                  'notificationResponseType': 0,
                  'payload': jsonEncode(initial),
                },
            },
            _ => null,
          };
        });
  });

  tearDown(() {
    debugDefaultTargetPlatformOverride = null;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, null);
  });

  test('비활성 게이트웨이는 플랫폼 SDK 없이 생성하고 사용할 수 있다', () async {
    final notifications = MobileNotifications.disabled();
    await notifications.show(_notification);

    expect(notifications.isPushConfigured, isFalse);
    expect(await notifications.getPushToken(), isNull);
    expect(notifications.takeInitialNotification(), isNull);
    expect(await notifications.foregroundMessages.toList(), isEmpty);
    expect(await notifications.openedNotifications.toList(), isEmpty);
    expect(await notifications.tokenChanges.toList(), isEmpty);
    expect(calls, isEmpty);
    await notifications.dispose();
  });

  test('Firebase 설정 없이도 로컬 알림을 표시하고 권한을 요청한다', () async {
    final notifications = await MobileNotifications.initialize();
    addTearDown(notifications.dispose);

    await notifications.show({..._notification, 'token': 'pairing-secret'});

    expect(notifications.isPushConfigured, isFalse);
    expect(await notifications.getPushToken(), isNull);
    expect(
      calls.any((call) => call.method == 'requestNotificationsPermission'),
      isTrue,
    );
    final call = calls.singleWhere((call) => call.method == 'show');
    final arguments = call.arguments as Map<dynamic, dynamic>;
    expect(arguments['title'], _notification['title']);
    expect(arguments['body'], _notification['body']);
    expect(jsonDecode(arguments['payload'] as String), _notification);
    expect(
      (arguments['platformSpecifics'] as Map)['channelId'],
      'kanvibe_task_status',
    );
  });

  test('로컬 알림으로 콜드 스타트하면 대상 태스크를 한 번만 꺼내 준다', () async {
    initial = _notification;
    final notifications = await MobileNotifications.initialize();
    addTearDown(notifications.dispose);

    expect(notifications.takeInitialNotification(), _notification);
    expect(notifications.takeInitialNotification(), isNull);
  });

  test('서로 다른 PC에서 같은 이벤트 식별자가 와도 알림을 각각 보존한다', () async {
    final notifications = await MobileNotifications.initialize();
    addTearDown(notifications.dispose);

    await notifications.show(_notification);
    await notifications.show({..._notification, 'connectionId': 'second:9736'});

    final displayed = calls.where((call) => call.method == 'show').toList();
    final firstId = (displayed.first.arguments as Map)['id'];
    final secondId = (displayed.last.arguments as Map)['id'];
    expect(firstId, isNot(secondId));
  });

  test('실행 중 로컬 알림 클릭을 전달하고 손상된 payload는 무시한다', () async {
    final notifications = await MobileNotifications.initialize();
    addTearDown(notifications.dispose);
    final opened = notifications.openedNotifications.first;

    Future<void> click(String payload) async {
      await TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .handlePlatformMessage(
            channel.name,
            const StandardMethodCodec().encodeMethodCall(
              MethodCall('didReceiveNotificationResponse', {
                'notificationId': 1,
                'notificationResponseType': 0,
                'payload': payload,
              }),
            ),
            (_) {},
          );
    }

    await click('not-json');
    await click(jsonEncode(_notification));

    expect(await opened, _notification);
  });
}
