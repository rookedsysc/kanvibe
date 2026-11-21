import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/l10n/app_localizations.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../application/notification_inbox_controller.dart';
import '../application/sync_review_cleanup_controller.dart';
import '../application/sync_review_controller.dart';
import 'sync_review_screen.dart';

class SyncReviewButton extends ConsumerStatefulWidget {
  const SyncReviewButton({super.key});

  @override
  ConsumerState<SyncReviewButton> createState() => _SyncReviewButtonState();
}

class _SyncReviewButtonState extends ConsumerState<SyncReviewButton> {
  bool _isReviewOpen = false;

  Future<void> _markRead(
    String notificationId,
    DesktopConnection connection,
  ) async {
    try {
      await ref
          .read(notificationInboxProvider.notifier)
          .markRead(notificationId);
    } catch (_) {
      if (!mounted) return;
      final active = ref
          .read(connectionControllerProvider)
          .value
          ?.activeConnection;
      if (active?.id == connection.id && active?.token == connection.token) {
        ScaffoldMessenger.maybeOf(context)?.showSnackBar(
          SnackBar(content: Text(context.tr('읽음 처리하지 못했습니다. 다시 시도해 주세요.'))),
        );
      }
    }
  }

  Future<void> _open(String notificationId) async {
    if (_isReviewOpen) return;
    final connection = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    if (connection == null) return;
    _isReviewOpen = true;
    unawaited(_markRead(notificationId, connection));
    try {
      await showSyncReviewDialog(context, notificationId);
    } finally {
      _isReviewOpen = false;
    }
  }

  @override
  Widget build(BuildContext context) {
    final notificationId = ref.watch(syncReviewNotificationIdProvider);
    final cleanup = ref.watch(syncReviewCleanupProvider);
    final review = notificationId == null
        ? null
        : ref.watch(syncReviewProvider(notificationId));
    final inbox = ref.watch(notificationInboxProvider);
    final hasError = review?.hasError == true || inbox.hasError;
    final isLoading = review?.isLoading == true || inbox.isLoading;
    final label = cleanup != null
        ? context.tr('정리 중')
        : hasError
        ? context.tr('탐지 항목 확인 실패')
        : isLoading
        ? context.tr('탐지 항목 확인 중…')
        : context.tr('탐지 항목 {count}개', {
            'count': review?.value?.detectedCount ?? 0,
          });
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Semantics(
          identifier: 'background-sync-review',
          child: TextButton.icon(
            onPressed: notificationId != null
                ? () => _open(notificationId)
                : hasError
                ? () => ref.invalidate(notificationInboxProvider)
                : null,
            icon: Icon(
              cleanup != null ? Icons.sync : Icons.fact_check_outlined,
            ),
            label: Text(label),
          ),
        ),
        if (cleanup != null)
          Text(
            context.tr('백그라운드에서 정리합니다. 검토 창을 닫아도 계속 진행됩니다.'),
            style: Theme.of(context).textTheme.bodySmall,
          ),
      ],
    );
  }
}
