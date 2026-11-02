import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'src/app.dart';
import 'src/features/notifications/application/mobile_notification_controller.dart';
import 'src/features/notifications/data/mobile_notifications.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  final notifications = await MobileNotifications.initialize();
  runApp(
    ProviderScope(
      overrides: [mobileNotificationsProvider.overrideWithValue(notifications)],
      child: const KanvibeMobileApp(),
    ),
  );
}
