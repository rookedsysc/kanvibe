import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'notification_settings_screen.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../../../common/providers.dart';
import '../../task_detail/presentation/task_detail_screen.dart';
import '../application/notification_inbox_controller.dart';
import '../application/notification_navigation_controller.dart';
import '../domain/inbox_notification.dart';

const _notificationButtonConstraints = BoxConstraints(
  minWidth: AppSizes.minTouchTarget,
  minHeight: AppSizes.minTouchTarget,
);

class NotificationCenterButton extends ConsumerWidget {
  const NotificationCenterButton({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final unread = ref.watch(
      notificationInboxProvider.select(
        (value) => value.value?.unreadCount ?? 0,
      ),
    );
    return Semantics(
      identifier: 'notification-center',
      child: IconButton(
        constraints: _notificationButtonConstraints,
        onPressed: () => Navigator.of(context).push(
          MaterialPageRoute<void>(
            builder: (_) => const NotificationCenterScreen(),
          ),
        ),
        tooltip: '알림',
        icon: Badge(
          isLabelVisible: unread > 0,
          label: Text(unread > 99 ? '99+' : '$unread'),
          child: const Icon(Icons.notifications_outlined),
        ),
      ),
    );
  }
}

class NotificationCenterScreen extends ConsumerStatefulWidget {
  const NotificationCenterScreen({super.key});

  @override
  ConsumerState<NotificationCenterScreen> createState() =>
      _NotificationCenterScreenState();
}

class _NotificationCenterScreenState
    extends ConsumerState<NotificationCenterScreen> {
  bool _unreadOnly = false;
  String? _openingId;
  int _openGeneration = 0;
  late final NotificationNavigationController _navigation;

  @override
  void initState() {
    super.initState();
    _navigation = ref.read(notificationNavigationControllerProvider);
  }

  Future<void> _markRead(InboxNotification notification) async {
    try {
      await ref
          .read(notificationInboxProvider.notifier)
          .markRead(notification.id);
    } catch (_) {
      _showError('읽음 처리하지 못했습니다. 다시 시도해 주세요.');
    }
  }

  Future<void> _markAllRead() async {
    try {
      await ref.read(notificationInboxProvider.notifier).markAllRead();
    } catch (_) {
      _showError('읽음 처리하지 못했습니다. 다시 시도해 주세요.');
    }
  }

  Future<void> _open(InboxNotification notification) async {
    if (_openingId != null) return;
    final connection = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    if (connection == null) return;
    final generation = ++_openGeneration;
    setState(() => _openingId = notification.id);
    try {
      final read = await ref
          .read(notificationInboxProvider.notifier)
          .markRead(notification.id);
      if (!mounted || !read || generation != _openGeneration) return;
      final current = ref
          .read(connectionControllerProvider)
          .value
          ?.activeConnection;
      if (current?.id != connection.id || current?.token != connection.token) {
        return;
      }
      final taskId = notification.taskId;
      if (taskId == null) return;
      final destination = await _navigation.prepareTaskDestination(
        connectionId: connection.id,
        taskId: taskId,
      );
      if (!mounted ||
          generation != _openGeneration ||
          destination == null ||
          ref
                  .read(connectionControllerProvider)
                  .value
                  ?.activeConnection
                  ?.token !=
              connection.token ||
          ref.read(connectionControllerProvider).value?.activeId !=
              connection.id) {
        return;
      }
      await Navigator.of(context).push(
        MaterialPageRoute<void>(
          builder: (_) => TaskDetailScreen(task: destination.task),
        ),
      );
    } on NotificationNavigationException catch (error) {
      _showError(error.message);
    } catch (_) {
      _showError('알림을 열지 못했습니다. 다시 시도해 주세요.');
    } finally {
      if (mounted && generation == _openGeneration) {
        setState(() => _openingId = null);
      }
    }
  }

  void _showError(String message) {
    if (mounted) {
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: Text(message)));
    }
  }

  @override
  void dispose() {
    _openGeneration += 1;
    _navigation.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final inbox = ref.watch(notificationInboxProvider);
    final connection = ref
        .watch(connectionControllerProvider)
        .value
        ?.activeConnection;
    final isUpdating = inbox.value?.isUpdating == true;
    return Scaffold(
      appBar: AppBar(
        title: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const Text('알림'),
            if (connection != null)
              Text(
                connection.displayName,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.labelSmall,
              ),
          ],
        ),
        actions: [
          IconButton(
            tooltip: '알림 설정',
            onPressed: connection == null
                ? null
                : () => Navigator.of(context).push(
                    MaterialPageRoute<void>(
                      builder: (_) => const NotificationSettingsScreen(),
                    ),
                  ),
            icon: const Icon(Icons.settings_outlined),
          ),
          Semantics(
            identifier: 'notifications-mark-all-read',
            child: IconButton(
              constraints: _notificationButtonConstraints,
              tooltip: '모두 읽음',
              onPressed: isUpdating || (inbox.value?.unreadCount ?? 0) == 0
                  ? null
                  : _markAllRead,
              icon: const Icon(Icons.done_all),
            ),
          ),
          IconButton(
            constraints: _notificationButtonConstraints,
            tooltip: '새로고침',
            onPressed: isUpdating
                ? null
                : () => ref.invalidate(notificationInboxProvider),
            icon: const Icon(Icons.refresh),
          ),
        ],
      ),
      body: SafeArea(
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: AppSizes.p16),
              child: Wrap(
                spacing: AppSizes.p8,
                children: [
                  Semantics(
                    identifier: 'notifications-all',
                    child: ChoiceChip(
                      label: const Text('전체'),
                      selectedColor: AppColors.brandSubtle,
                      checkmarkColor: AppColors.brandPrimary,
                      labelStyle: const TextStyle(color: AppColors.textPrimary),
                      selected: !_unreadOnly,
                      onSelected: (_) => setState(() => _unreadOnly = false),
                    ),
                  ),
                  Semantics(
                    identifier: 'notifications-unread',
                    child: ChoiceChip(
                      label: const Text('안 읽음'),
                      selectedColor: AppColors.brandSubtle,
                      checkmarkColor: AppColors.brandPrimary,
                      labelStyle: const TextStyle(color: AppColors.textPrimary),
                      selected: _unreadOnly,
                      onSelected: (_) => setState(() => _unreadOnly = true),
                    ),
                  ),
                ],
              ),
            ),
            if (isUpdating) const LinearProgressIndicator(),
            Expanded(
              child: inbox.when(
                loading: () => const Center(child: CircularProgressIndicator()),
                error: (_, _) => Center(
                  child: Padding(
                    padding: const EdgeInsets.all(AppSizes.p24),
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        const Text(
                          '알림을 불러오지 못했습니다.\nPC 연결과 데스크톱 앱 버전을 확인해 주세요.',
                          textAlign: TextAlign.center,
                        ),
                        const SizedBox(height: AppSizes.p16),
                        FilledButton(
                          onPressed: () =>
                              ref.invalidate(notificationInboxProvider),
                          child: const Text('다시 시도'),
                        ),
                      ],
                    ),
                  ),
                ),
                data: (snapshot) {
                  final visible = snapshot.notifications
                      .where((entry) => !_unreadOnly || !entry.isRead)
                      .toList();
                  return RefreshIndicator(
                    onRefresh: () =>
                        ref.refresh(notificationInboxProvider.future),
                    child: ListView.separated(
                      physics: const AlwaysScrollableScrollPhysics(),
                      padding: const EdgeInsets.all(AppSizes.p16),
                      itemCount: visible.isEmpty ? 1 : visible.length,
                      separatorBuilder: (_, _) => const Divider(),
                      itemBuilder: (context, index) => visible.isEmpty
                          ? Padding(
                              padding: const EdgeInsets.all(AppSizes.p24),
                              child: Center(
                                child: Text(
                                  _unreadOnly ? '읽지 않은 알림이 없습니다.' : '알림이 없습니다.',
                                ),
                              ),
                            )
                          : _NotificationRow(
                              key: ValueKey(visible[index].id),
                              notification: visible[index],
                              isOpening: _openingId == visible[index].id,
                              onOpen: isUpdating || _openingId != null
                                  ? null
                                  : () => _open(visible[index]),
                              onMarkRead: isUpdating
                                  ? null
                                  : () => _markRead(visible[index]),
                            ),
                    ),
                  );
                },
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _NotificationRow extends StatelessWidget {
  const _NotificationRow({
    super.key,
    required this.notification,
    required this.isOpening,
    required this.onOpen,
    required this.onMarkRead,
  });

  final InboxNotification notification;
  final bool isOpening;
  final VoidCallback? onOpen;
  final VoidCallback? onMarkRead;

  @override
  Widget build(BuildContext context) {
    final date = notification.createdAt.toLocal();
    final localizations = MaterialLocalizations.of(context);
    final timestamp =
        '${localizations.formatShortDate(date)} ${localizations.formatTimeOfDay(TimeOfDay.fromDateTime(date))}';
    return Semantics(
      identifier: 'notification-${notification.id}',
      child: ListTile(
        contentPadding: EdgeInsets.zero,
        onTap: onOpen,
        leading: Icon(
          notification.isRead
              ? Icons.notifications_none
              : Icons.mark_email_unread_outlined,
          color: notification.isRead
              ? AppColors.textSecondary
              : AppColors.brandPrimary,
        ),
        title: Text(
          notification.title,
          style: TextStyle(
            fontWeight: notification.isRead
                ? FontWeight.normal
                : FontWeight.w600,
          ),
        ),
        subtitle: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(notification.body),
            const SizedBox(height: AppSizes.p4),
            Text(timestamp, style: Theme.of(context).textTheme.labelSmall),
            if (notification.taskId == null) const Text('데스크톱에서 확인'),
          ],
        ),
        trailing: isOpening
            ? const SizedBox(
                width: AppSizes.p24,
                height: AppSizes.p24,
                child: CircularProgressIndicator(),
              )
            : notification.isRead
            ? null
            : Semantics(
                identifier: 'notification-mark-read-${notification.id}',
                child: IconButton(
                  constraints: _notificationButtonConstraints,
                  tooltip: '읽음 처리',
                  onPressed: onMarkRead,
                  icon: const Icon(Icons.done),
                ),
              ),
      ),
    );
  }
}
