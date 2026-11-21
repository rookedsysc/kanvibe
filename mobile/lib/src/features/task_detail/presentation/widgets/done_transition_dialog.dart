import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/l10n/app_localizations.dart';
import '../../data/task_details_api.dart';
import '../../../../common/providers.dart';

Future<bool> confirmDoneTransition(
  BuildContext context,
  WidgetRef ref,
  String taskId,
) async {
  final connection = ref
      .read(connectionControllerProvider)
      .value
      ?.activeConnection;
  if (connection == null) return false;
  final api = TaskDetailsApi(ref.read(desktopClientProvider), connection);
  bool isCurrent() {
    final current = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    return current?.id == connection.id && current?.token == connection.token;
  }

  try {
    if (await api.isDoneNoticeDismissed()) {
      return context.mounted && isCurrent();
    }
  } catch (_) {
    if (!context.mounted) return false;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(context.tr('Done 설정을 확인하지 못했습니다. 다시 시도해 주세요.'))),
    );
    return false;
  }
  if (!context.mounted || !isCurrent()) return false;
  var dismiss = false;
  var saving = false;
  String? error;
  return await showDialog<bool>(
        context: context,
        builder: (dialogContext) => StatefulBuilder(
          builder: (context, setDialogState) => AlertDialog(
            title: Text(context.tr('Done으로 옮길까요?')),
            content: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(
                  context.tr(
                    'Done으로 옮겨도 worktree·브랜치·세션은 유지됩니다. 리소스를 정리하려면 태스크를 삭제해 주세요.',
                  ),
                ),
                CheckboxListTile(
                  contentPadding: EdgeInsets.zero,
                  value: dismiss,
                  title: Text(context.tr('다시 묻지 않기')),
                  onChanged: saving
                      ? null
                      : (value) =>
                            setDialogState(() => dismiss = value ?? false),
                ),
                if (error != null)
                  Text(
                    error!,
                    style: TextStyle(
                      color: Theme.of(context).colorScheme.error,
                    ),
                  ),
              ],
            ),
            actions: [
              TextButton(
                onPressed: saving
                    ? null
                    : () => Navigator.pop(dialogContext, false),
                child: Text(context.tr('취소')),
              ),
              FilledButton(
                onPressed: saving
                    ? null
                    : () async {
                        setDialogState(() => saving = true);
                        try {
                          if (!isCurrent()) {
                            if (dialogContext.mounted) {
                              Navigator.pop(dialogContext, false);
                            }
                            return;
                          }
                          if (dismiss) await api.dismissDoneNotice();
                          if (dialogContext.mounted) {
                            Navigator.pop(dialogContext, isCurrent());
                          }
                        } catch (_) {
                          if (dialogContext.mounted) {
                            setDialogState(() {
                              saving = false;
                              error = context.tr('설정을 저장하지 못했습니다. 다시 시도해 주세요.');
                            });
                          }
                        }
                      },
                child: Text(context.tr(saving ? '저장하는 중…' : 'Done으로 옮기기')),
              ),
            ],
          ),
        ),
      ) ??
      false;
}
