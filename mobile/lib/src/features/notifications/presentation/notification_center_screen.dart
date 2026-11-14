import 'sync_review_screen.dart';
import '../../../common/l10n/app_localizations.dart';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'notification_settings_screen.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../../../common/providers.dart';
import '../../task_detail/presentation/task_detail_screen.dart';
import '../application/notification_inbox_controller.dart';
import '../application/background_sync_controller.dart';
import '../application/notification_navigation_controller.dart';
import '../domain/inbox_notification.dart';
import 'background_sync_card.dart';

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
    final connection = ref.watch(
      connectionControllerProvider.select(
        (value) => value.value?.activeConnection,
      ),
    );
    final syncRunning = ref.watch(
      backgroundSyncProvider.select((value) {
        final snapshot = value.value;
        return snapshot?.belongsTo(connection) == true &&
            (snapshot!.isStarting || snapshot.status.isRunning);
      }),
    );
    return Semantics(
      identifier: 'notification-center',
      child: IconButton(
        constraints: _notificationButtonConstraints,
        onPressed: () => Navigator.of(context).push(
          MaterialPageRoute<void>(builder: (_) => NotificationCenterScreen()),
        ),
        tooltip: context.tr('알림'),
        icon: Badge(
          isLabelVisible: unread > 0 || syncRunning,
          label: syncRunning
              ? Icon(
                  Icons.sync,
                  size: 12,
                  semanticLabel: context.tr('백그라운드 동기화 진행 중'),
                )
              : Text(unread > 99 ? '99+' : '$unread'),
          child: Icon(Icons.notifications_outlined),
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
      if (notification.hasSyncReview) {
        setState(() => _openingId = null);
        await showSyncReviewDialog(context, notification.id);
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
          .showSnackBar(SnackBar(content: Text(context.tr(message))));
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
            Text(context.tr('알림')),
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
            tooltip: context.tr('알림 설정'),
            onPressed: connection == null
                ? null
                : () => Navigator.of(context).push(
                    MaterialPageRoute<void>(
                      builder: (_) => NotificationSettingsScreen(),
                    ),
                  ),
            icon: Icon(Icons.settings_outlined),
          ),
          Semantics(
            identifier: 'notifications-mark-all-read',
            child: IconButton(
              constraints: _notificationButtonConstraints,
              tooltip: context.tr('모두 읽음'),
              onPressed: isUpdating || (inbox.value?.unreadCount ?? 0) == 0
                  ? null
                  : _markAllRead,
              icon: Icon(Icons.done_all),
            ),
          ),
          IconButton(
            constraints: _notificationButtonConstraints,
            tooltip: context.tr('새로고침'),
            onPressed: isUpdating
                ? null
                : () => ref.invalidate(notificationInboxProvider),
            icon: Icon(Icons.refresh),
          ),
        ],
      ),
      body: SafeArea(
        child: RefreshIndicator(
          onRefresh: () => ref.refresh(notificationInboxProvider.future),
          child: CustomScrollView(
            physics: const AlwaysScrollableScrollPhysics(),
            slivers: [
              SliverToBoxAdapter(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    const Padding(
                      padding: EdgeInsets.fromLTRB(
                        AppSizes.p12,
                        AppSizes.p8,
                        AppSizes.p12,
                        AppSizes.p8,
                      ),
                      child: BackgroundSyncCard(),
                    ),
                    Padding(
                      padding: EdgeInsets.symmetric(horizontal: AppSizes.p16),
                      child: Wrap(
                        spacing: AppSizes.p8,
                        children: [
                          Semantics(
                            identifier: 'notifications-all',
                            child: ChoiceChip(
                              label: Text(context.tr('전체')),
                              selectedColor: AppColors.brandSubtle,
                              checkmarkColor: AppColors.brandPrimary,
                              labelStyle: TextStyle(
                                color: context.colors.textPrimary,
                              ),
                              selected: !_unreadOnly,
                              onSelected: (_) =>
                                  setState(() => _unreadOnly = false),
                            ),
                          ),
                          Semantics(
                            identifier: 'notifications-unread',
                            child: ChoiceChip(
                              label: Text(context.tr('안 읽음')),
                              selectedColor: AppColors.brandSubtle,
                              checkmarkColor: AppColors.brandPrimary,
                              labelStyle: TextStyle(
                                color: context.colors.textPrimary,
                              ),
                              selected: _unreadOnly,
                              onSelected: (_) =>
                                  setState(() => _unreadOnly = true),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ],
                ),
              ),
              if (isUpdating)
                const SliverToBoxAdapter(child: LinearProgressIndicator()),
              inbox.when(
                loading: () => const SliverFillRemaining(
                  hasScrollBody: false,
                  child: Center(child: CircularProgressIndicator()),
                ),
                error: (_, _) => SliverFillRemaining(
                  hasScrollBody: false,
                  child: Center(
                    child: Padding(
                      padding: const EdgeInsets.all(AppSizes.p24),
                      child: Column(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Text(
                            context.tr(
                              '알림을 불러오지 못했습니다.\nPC 연결과 데스크톱 앱 버전을 확인해 주세요.',
                            ),
                            textAlign: TextAlign.center,
                          ),
                          const SizedBox(height: AppSizes.p16),
                          FilledButton(
                            onPressed: () =>
                                ref.invalidate(notificationInboxProvider),
                            child: Text(context.tr('다시 시도')),
                          ),
                        ],
                      ),
                    ),
                  ),
                ),
                data: (snapshot) {
                  final visible = snapshot.notifications
                      .where((entry) => !_unreadOnly || !entry.isRead)
                      .toList();
                  return SliverPadding(
                    padding: const EdgeInsets.all(AppSizes.p16),
                    sliver: SliverList.separated(
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
            ],
          ),
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
              ? context.colors.textSecondary
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
            SizedBox(height: AppSizes.p4),
            Text(timestamp, style: Theme.of(context).textTheme.labelSmall),
            if (notification.hasSyncReview)
              Text(context.tr('동기화 결과 검토'))
            else if (notification.taskId == null)
              Text(context.tr('데스크톱에서 확인')),
          ],
        ),
        trailing: isOpening
            ? SizedBox(
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
                  tooltip: context.tr('읽음 처리'),
                  onPressed: onMarkRead,
                  icon: Icon(Icons.done),
                ),
              ),
      ),
    );
  }
}
