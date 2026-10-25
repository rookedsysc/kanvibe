import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'common/constants/app_theme.dart';
import 'common/providers.dart';
import 'features/board/presentation/board_screen.dart';
import 'features/connection/presentation/connection_screen.dart';
import 'features/connection/presentation/desktop_connections_screen.dart';
import 'features/notifications/application/mobile_notification_controller.dart';
import 'features/notifications/application/notification_navigation_controller.dart';
import 'features/notifications/domain/task_notification.dart';
import 'features/task_detail/presentation/task_detail_screen.dart';

/// 앱 전체. 저장된 연결이 있으면 보드로, 없으면 연결 화면으로 연다
class KanvibeMobileApp extends ConsumerStatefulWidget {
  const KanvibeMobileApp({super.key});

  @override
  ConsumerState<KanvibeMobileApp> createState() => _KanvibeMobileAppState();
}

class _KanvibeMobileAppState extends ConsumerState<KanvibeMobileApp>
    with WidgetsBindingObserver {
  final _navigatorKey = GlobalKey<NavigatorState>();
  final _messengerKey = GlobalKey<ScaffoldMessengerState>();
  late final MobileNotificationController _notifications;
  late final NotificationNavigationController _navigation;
  late final StreamSubscription<TaskNotification> _openSubscription;
  int _openGeneration = 0;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _notifications = ref.read(mobileNotificationControllerProvider);
    _navigation = ref.read(notificationNavigationControllerProvider);
    _openSubscription = _notifications.openRequests.listen(_openNotification);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      final initial = _notifications.takeInitialNotification();
      if (initial != null) unawaited(_openNotification(initial));
    });
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) {
      _notifications.setActive(true);
    } else if (state == AppLifecycleState.paused ||
        state == AppLifecycleState.hidden ||
        state == AppLifecycleState.detached) {
      _notifications.setActive(false);
    }
  }

  Future<void> _openNotification(TaskNotification notification) async {
    final generation = ++_openGeneration;
    try {
      final destination = await _navigation.prepareDestination(notification);
      if (!mounted || generation != _openGeneration || destination == null) {
        return;
      }
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted ||
            generation != _openGeneration ||
            ref.read(connectionControllerProvider).value?.activeId !=
                destination.connection.id) {
          return;
        }
        final navigator = _navigatorKey.currentState;
        if (navigator == null) return;
        navigator.popUntil((route) => route.isFirst);
        unawaited(
          navigator.push(
            MaterialPageRoute<void>(
              builder: (_) => TaskDetailScreen(
                key: ValueKey(
                  '${destination.connection.id}/${destination.task.id}',
                ),
                task: destination.task,
              ),
            ),
          ),
        );
      });
      WidgetsBinding.instance.ensureVisualUpdate();
    } on NotificationNavigationException catch (error) {
      if (!mounted || generation != _openGeneration) return;
      _messengerKey.currentState?.showSnackBar(
        SnackBar(
          content: Text(error.message),
          action: SnackBarAction(
            label: '다시 시도',
            onPressed: () => _openNotification(notification),
          ),
        ),
      );
    }
  }

  @override
  void dispose() {
    _openGeneration += 1;
    _navigation.cancel();
    WidgetsBinding.instance.removeObserver(this);
    unawaited(_openSubscription.cancel());
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final connections = ref.watch(connectionControllerProvider);

    return MaterialApp(
      navigatorKey: _navigatorKey,
      scaffoldMessengerKey: _messengerKey,
      title: 'kanvibe',
      debugShowCheckedModeBanner: false,
      theme: buildAppTheme(),
      home: connections.when(
        loading: () =>
            const Scaffold(body: Center(child: CircularProgressIndicator())),
        error: (_, _) => const ConnectionScreen(),
        data: (data) => data.connections.isEmpty
            ? const ConnectionScreen()
            : data.activeConnection == null
            ? const DesktopConnectionsScreen()
            : const BoardScreen(),
      ),
    );
  }
}
