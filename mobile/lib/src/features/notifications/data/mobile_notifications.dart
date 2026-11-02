import 'dart:async';
import 'dart:convert';

import 'package:firebase_core/firebase_core.dart';
import 'package:firebase_messaging/firebase_messaging.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';

/// 플랫폼 알림 SDK의 메시지와 클릭을 앱에서 쓰는 JSON으로 바꾼다.
class MobileNotifications {
  MobileNotifications._()
    : _local = FlutterLocalNotificationsPlugin(),
      _opened = StreamController<Map<String, dynamic>>.broadcast();

  /// 테스트와 모바일 이외의 실행에서는 플랫폼 채널을 열지 않는다.
  MobileNotifications.disabled() : _local = null, _opened = null;

  final FlutterLocalNotificationsPlugin? _local;
  final StreamController<Map<String, dynamic>>? _opened;
  FirebaseMessaging? _messaging;
  StreamSubscription<RemoteMessage>? _openedSubscription;
  Map<String, dynamic>? _initial;
  bool _localReady = false;
  bool _disposed = false;

  static const _channel = AndroidNotificationChannel(
    'kanvibe_task_status',
    'KanVibe task status',
    description: 'Hook status updates for your KanVibe tasks',
    importance: Importance.high,
  );

  static const _payloadFields = {
    'type',
    'eventId',
    'taskId',
    'title',
    'body',
    'status',
    'createdAt',
    'connectionId',
  };

  String get platform =>
      defaultTargetPlatform == TargetPlatform.iOS ? 'ios' : 'android';

  bool get isPushConfigured => _messaging != null;

  Stream<Map<String, dynamic>> get foregroundMessages => _messaging == null
      ? const Stream.empty()
      : FirebaseMessaging.onMessage.map((message) => _payload(message.data));

  Stream<Map<String, dynamic>> get openedNotifications =>
      _opened?.stream ?? const Stream.empty();

  Stream<String> get tokenChanges =>
      // 데스크탑의 기존 FCM 등록 토큰 계약에 맞춰 갱신도 같은 API로 받는다.
      // ignore: deprecated_member_use
      _messaging?.onTokenRefresh ?? const Stream.empty();

  static Future<MobileNotifications> initialize() async {
    if (kIsWeb ||
        (defaultTargetPlatform != TargetPlatform.android &&
            defaultTargetPlatform != TargetPlatform.iOS)) {
      return MobileNotifications.disabled();
    }

    final notifications = MobileNotifications._();
    try {
      await notifications._initializeLocal();
    } catch (error) {
      _logFailure('Local notification initialization', error);
    }

    final options = _firebaseOptions();
    if (options == null) {
      debugPrint(
        'KanVibe push notifications disabled: Firebase build configuration is '
        'missing. Local WebSocket notifications remain available.',
      );
      return notifications;
    }

    try {
      await Firebase.initializeApp(options: options);
      await notifications._initializePush();
    } catch (error) {
      _logFailure('Push notification initialization', error);
    }
    return notifications;
  }

  Future<void> _initializeLocal() async {
    final local = _local!;
    await local.initialize(
      settings: const InitializationSettings(
        android: AndroidInitializationSettings('ic_stat_kanvibe'),
        iOS: DarwinInitializationSettings(),
      ),
      onDidReceiveNotificationResponse: (response) {
        final payload = _readPayload(response.payload);
        if (payload != null) _receiveOpened(payload);
      },
    );
    _localReady = true;
    final android = local
        .resolvePlatformSpecificImplementation<
          AndroidFlutterLocalNotificationsPlugin
        >();
    await android?.createNotificationChannel(_channel);
    await android?.requestNotificationsPermission();

    final launch = await local.getNotificationAppLaunchDetails();
    if (launch?.didNotificationLaunchApp == true) {
      _initial = _readPayload(launch?.notificationResponse?.payload);
    }
  }

  Future<void> _initializePush() async {
    final messaging = FirebaseMessaging.instance;
    _messaging = messaging;
    // 포그라운드는 WebSocket과 푸시를 합쳐 로컬 알림 하나만 표시한다.
    await messaging.setForegroundNotificationPresentationOptions();
    _openedSubscription = FirebaseMessaging.onMessageOpenedApp.listen(
      (message) => _receiveOpened(_payload(message.data)),
    );
    final initialMessage = await messaging.getInitialMessage();
    if (initialMessage != null) {
      _initial ??= _payload(initialMessage.data);
    }
    await messaging.requestPermission();
  }

  Map<String, dynamic>? takeInitialNotification() {
    final initial = _initial;
    _initial = null;
    return initial;
  }

  Future<String?> getPushToken() async {
    final messaging = _messaging;
    if (messaging == null || _disposed) return null;
    try {
      final settings = await messaging.getNotificationSettings();
      if (settings.authorizationStatus != AuthorizationStatus.authorized &&
          settings.authorizationStatus != AuthorizationStatus.provisional) {
        return null;
      }
      if (platform == 'ios' && await messaging.getAPNSToken() == null) {
        return null;
      }
      // 데스크탑의 FCM HTTP v1 발송 계약은 FID가 아닌 기존 등록 토큰을 받는다.
      // ignore: deprecated_member_use
      return await messaging.getToken();
    } catch (error) {
      _logFailure('Push token retrieval', error);
      return null;
    }
  }

  Future<void> show(Map<String, dynamic> notification) async {
    if (!_localReady || _disposed) return;
    final payload = _payload(notification);
    final eventId = payload['eventId'] as String? ?? '';
    final connectionId = payload['connectionId'] as String? ?? '';
    await _local!.show(
      id: '$connectionId:$eventId'.hashCode & 0x7fffffff,
      title: payload['title'] as String? ?? 'KanVibe',
      body: payload['body'] as String?,
      notificationDetails: NotificationDetails(
        android: AndroidNotificationDetails(
          _channel.id,
          _channel.name,
          channelDescription: _channel.description,
          importance: Importance.high,
          priority: Priority.high,
          icon: 'ic_stat_kanvibe',
        ),
        iOS: const DarwinNotificationDetails(),
      ),
      payload: jsonEncode(payload),
    );
  }

  void _receiveOpened(Map<String, dynamic> notification) {
    if (_disposed) return;
    if (_opened?.hasListener == true) {
      _opened!.add(notification);
    } else {
      _initial = notification;
    }
  }

  Future<void> dispose() async {
    if (_disposed) return;
    _disposed = true;
    await _openedSubscription?.cancel();
    await _opened?.close();
  }

  static Map<String, dynamic> _payload(Map<String, dynamic> notification) => {
    for (final field in _payloadFields)
      if (notification[field] is String) field: notification[field],
  };

  static Map<String, dynamic>? _readPayload(String? payload) {
    if (payload == null || payload.isEmpty) return null;
    try {
      final decoded = jsonDecode(payload);
      return decoded is Map<String, dynamic> ? _payload(decoded) : null;
    } on FormatException {
      return null;
    }
  }

  static FirebaseOptions? _firebaseOptions() {
    const apiKey = String.fromEnvironment('KANVIBE_FIREBASE_API_KEY');
    const appId = String.fromEnvironment('KANVIBE_FIREBASE_APP_ID');
    const senderId = String.fromEnvironment(
      'KANVIBE_FIREBASE_MESSAGING_SENDER_ID',
    );
    const projectId = String.fromEnvironment('KANVIBE_FIREBASE_PROJECT_ID');
    const iosBundleId = String.fromEnvironment(
      'KANVIBE_FIREBASE_IOS_BUNDLE_ID',
      defaultValue: 'com.kanvibe.kanvibeMobile',
    );
    if ([apiKey, appId, senderId, projectId].any((value) => value.isEmpty)) {
      return null;
    }
    return FirebaseOptions(
      apiKey: apiKey,
      appId: appId,
      messagingSenderId: senderId,
      projectId: projectId,
      iosBundleId: defaultTargetPlatform == TargetPlatform.iOS
          ? iosBundleId
          : null,
    );
  }

  static void _logFailure(String operation, Object error) {
    final reason = error is FirebaseException ? error.code : error.runtimeType;
    debugPrint('KanVibe $operation failed ($reason).');
  }
}
