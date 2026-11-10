import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/constants/app_sizes.dart';
import '../../../../common/network/desktop_client.dart';
import '../../../../common/providers.dart';
import '../../../task_create/domain/session_type.dart';

/// 태스크의 터미널 세션 종류를 바꾸는 시트.
///
/// 실제 전환은 데스크탑이 하고 기기는 그 결과를 따른다. 기존 세션이 켜져 있으면 닫을지 먼저 묻고,
/// 남겨 두기를 고르면 기존 세션은 데스크탑 백그라운드에 그대로 둔다.
/// 바꾸기에 성공하면 고른 종류를 돌려주며 닫힌다. 실패하면 닫지 않고 사유를 남긴다.
class TaskSessionTypeSheet extends ConsumerStatefulWidget {
  const TaskSessionTypeSheet({
    required this.taskId,
    required this.currentSessionType,
    super.key,
  });

  final String taskId;
  final SessionType currentSessionType;

  @override
  ConsumerState<TaskSessionTypeSheet> createState() =>
      _TaskSessionTypeSheetState();
}

class _TaskSessionTypeSheetState extends ConsumerState<TaskSessionTypeSheet> {
  bool _isChanging = false;
  String? _failureMessage;

  Future<void> _change(SessionType sessionType) async {
    final connection = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    if (connection == null) {
      return;
    }

    setState(() {
      _isChanging = true;
      _failureMessage = null;
    });

    final client = ref.read(desktopClientProvider);
    try {
      final isExistingSessionAlive = await client.fetchTaskSessionAlive(
        connection,
        widget.taskId,
      );
      final shouldCloseExistingSession = isExistingSessionAlive
          ? await _askToCloseExistingSession()
          : false;
      if (shouldCloseExistingSession == null) {
        if (mounted) setState(() => _isChanging = false);
        return;
      }

      await client.changeTaskSessionType(
        connection,
        widget.taskId,
        sessionType: sessionType,
        shouldCloseExistingSession: shouldCloseExistingSession,
      );
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _isChanging = false;
        _failureMessage = _changeFailureMessage(error);
      });
      return;
    }

    /// 데스크탑이 이전 스트림을 끊기 전이라도 새 세션의 pane을 바로 받아 화면을 맞춘다
    ref.invalidate(surfacesProvider(widget.taskId));

    if (mounted) {
      Navigator.of(context).pop(sessionType);
    }
  }

  /// 기존 세션을 닫을지 묻는다. 창을 닫고 물러나면 null이라 바꾸지 않는다
  Future<bool?> _askToCloseExistingSession() => showDialog<bool>(
    context: context,
    builder: (dialogContext) => AlertDialog(
      title: const Text('기존 세션 닫기'),
      content: Text(
        '켜져 있는 기존 ${widget.currentSessionType.wireValue} 세션을 닫을까요?\n'
        '남겨 두면 데스크탑 백그라운드에서 계속 실행됩니다.',
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(dialogContext).pop(false),
          child: const Text('남겨 두기'),
        ),
        FilledButton(
          onPressed: () => Navigator.of(dialogContext).pop(true),
          child: const Text('닫기'),
        ),
      ],
    ),
  );

  String _changeFailureMessage(Object error) {
    if (error is! DesktopRequestException) {
      return '데스크탑에 닿지 못했습니다. 잠시 뒤 다시 시도해 주세요.';
    }
    if (error.statusCode == 404) {
      return '데스크탑에서 이미 지워진 태스크입니다.';
    }
    return error.message ?? '세션 종류를 바꾸지 못했습니다.';
  }

  @override
  Widget build(BuildContext context) {
    final targets = SessionType.values
        .where((sessionType) => sessionType != widget.currentSessionType)
        .toList();

    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(
          AppSizes.p24,
          AppSizes.p16,
          AppSizes.p24,
          AppSizes.p24,
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text('세션 종류 바꾸기', style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: AppSizes.p8),
            Text(
              '지금은 ${widget.currentSessionType.wireValue}입니다.',
              style: Theme.of(context).textTheme.bodySmall,
            ),
            const SizedBox(height: AppSizes.p16),
            for (final sessionType in targets) ...[
              OutlinedButton(
                onPressed: _isChanging ? null : () => _change(sessionType),
                style: OutlinedButton.styleFrom(
                  minimumSize: const Size.fromHeight(AppSizes.minTouchTarget),
                ),
                child: Text('${sessionType.wireValue}(으)로 바꾸기'),
              ),
              const SizedBox(height: AppSizes.p8),
            ],
            if (_failureMessage != null) ...[
              const SizedBox(height: AppSizes.p8),
              Text(
                _failureMessage!,
                style: Theme.of(context).textTheme.bodySmall
                    ?.copyWith(color: Theme.of(context).colorScheme.error),
              ),
            ],
          ],
        ),
      ),
    );
  }
}
