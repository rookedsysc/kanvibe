import '../../../../common/l10n/app_localizations.dart';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/constants/app_sizes.dart';
import '../../../../common/providers.dart';
import '../../../task_create/domain/session_type.dart';
import '../../application/task_mutation.dart';
import 'busy_button_label.dart';

/// 태스크의 터미널 세션 종류를 바꾸는 시트.
///
/// 실제 전환은 데스크탑이 하고 기기는 그 결과를 따른다. 기존 세션이 켜져 있으면 닫을지 먼저 묻고,
/// 남겨 두기를 고르면 기존 세션은 데스크탑 백그라운드에 그대로 둔다.
/// 바꾸는 요청은 `taskMutationProvider`가 들고 있어 시트를 먼저 닫아도 끝까지 반영된다.
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
  /// 기존 세션이 켜져 있는지 묻는 동안 고른 종류. 실제 변경 진행은 provider가 들고 있다
  SessionType? _checkingSessionType;
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
      _checkingSessionType = sessionType;
      _failureMessage = null;
    });

    final bool isExistingSessionAlive;
    try {
      isExistingSessionAlive = await ref
          .read(desktopClientProvider)
          .fetchTaskSessionAlive(connection, widget.taskId);
    } catch (error) {
      if (mounted) {
        setState(() {
          _checkingSessionType = null;
          _failureMessage = sessionTypeChangeFailureMessage(error);
        });
      }
      return;
    }
    if (!mounted) {
      return;
    }
    // 확인이 끝났으니 진행 표시를 거두고, 필요하면 사용자의 답을 기다린다
    setState(() => _checkingSessionType = null);
    final shouldCloseExistingSession = isExistingSessionAlive
        ? await _askToCloseExistingSession()
        : false;
    if (!mounted) {
      return;
    }
    if (shouldCloseExistingSession == null) {
      return;
    }

    final outcome = await ref
        .read(taskMutationProvider.notifier)
        .changeSessionType(
          widget.taskId,
          sessionType,
          shouldCloseExistingSession: shouldCloseExistingSession,
        );
    if (!mounted) {
      return;
    }
    switch (outcome) {
      case TaskMutationSucceeded():
        Navigator.of(context).pop(sessionType);
      case TaskMutationFailed(:final message):
        setState(() => _failureMessage = message);
      case TaskMutationSkipped():
        break;
    }
  }

  /// 기존 세션을 닫을지 묻는다. 창을 닫고 물러나면 null이라 바꾸지 않는다
  Future<bool?> _askToCloseExistingSession() => showDialog<bool>(
    context: context,
    builder: (dialogContext) => AlertDialog(
      title: Text(context.tr('기존 세션 닫기')),
      content: Text(
        context.tr('켜져 있는 기존 {type} 세션을 닫을까요?\n', {
              'type': widget.currentSessionType.wireValue,
            }) +
            context.tr('남겨 두면 데스크탑 백그라운드에서 계속 실행됩니다.'),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(dialogContext).pop(false),
          child: Text(context.tr('남겨 두기')),
        ),
        FilledButton(
          onPressed: () => Navigator.of(dialogContext).pop(true),
          child: Text(context.tr('닫기')),
        ),
      ],
    ),
  );

  @override
  Widget build(BuildContext context) {
    final targets = SessionType.values
        .where((sessionType) => sessionType != widget.currentSessionType)
        .toList();
    final changingSessionType = ref.watch(
      taskMutationProvider.select((progress) {
        final taskProgress = progress[widget.taskId];
        return taskProgress is TaskSessionTypeChanging
            ? taskProgress.sessionType
            : null;
      }),
    );
    final isChanging =
        _checkingSessionType != null || changingSessionType != null;

    return SafeArea(
      child: Padding(
        padding: EdgeInsets.fromLTRB(
          AppSizes.p24,
          AppSizes.p16,
          AppSizes.p24,
          AppSizes.p24,
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              context.tr('세션 종류 바꾸기'),
              style: Theme.of(context).textTheme.titleMedium,
            ),
            SizedBox(height: AppSizes.p8),
            Text(
              context.tr('지금은 {type}입니다.', {
                'type': widget.currentSessionType.wireValue,
              }),
              style: Theme.of(context).textTheme.bodySmall,
            ),
            SizedBox(height: AppSizes.p16),
            for (final sessionType in targets) ...[
              OutlinedButton(
                onPressed: isChanging ? null : () => _change(sessionType),
                style: OutlinedButton.styleFrom(
                  minimumSize: Size.fromHeight(AppSizes.minTouchTarget),
                ),
                child: BusyButtonLabel(
                  label: context.tr('{type}(으)로 바꾸기', {
                    'type': sessionType.wireValue,
                  }),
                  busyLabel: _checkingSessionType == sessionType
                      ? context.tr('기존 세션을 확인하는 중…')
                      : context.tr('세션을 준비하는 중…'),
                  isBusy:
                      _checkingSessionType == sessionType ||
                      changingSessionType == sessionType,
                ),
              ),
              SizedBox(height: AppSizes.p8),
            ],
            if (_failureMessage != null) ...[
              SizedBox(height: AppSizes.p8),
              Text(
                context.tr(_failureMessage!),
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
