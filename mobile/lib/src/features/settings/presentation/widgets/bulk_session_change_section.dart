import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/constants/app_sizes.dart';
import '../../../../common/l10n/app_localizations.dart';
import '../../../../common/providers.dart';
import '../../../task_create/domain/session_type.dart';
import '../../application/pc_settings_controller.dart';
import '../../domain/pc_settings.dart';
import 'pc_request_failure.dart';

class BulkSessionChangeSection extends ConsumerStatefulWidget {
  const BulkSessionChangeSection({super.key});
  @override
  ConsumerState<BulkSessionChangeSection> createState() =>
      _BulkSessionChangeSectionState();
}

class _BulkSessionChangeSectionState
    extends ConsumerState<BulkSessionChangeSection> {
  SessionType _target = SessionType.herdr;
  bool _closeExisting = false;
  Future<void> _apply(SessionChangeTargets targets) async {
    final before = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    final target = _target;
    final close = _closeExisting;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(context.tr('기존 태스크의 세션을 일괄 변경할까요?')),
        content: Text(
          context.tr(
            '{count}개 태스크를 {type}(으)로 변경합니다. 실행 중인 이전 세션 {live}개는 {action}.',
            {
              'count': targets.taskCount,
              'type': target.wireValue,
              'live': targets.liveSessionCount,
              'action': context.tr(close ? '종료합니다' : '유지합니다'),
            },
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(context.tr('취소')),
          ),
          FilledButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(context.tr('일괄 변경')),
          ),
        ],
      ),
    );
    if (!mounted || confirmed != true) return;
    final active = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    if (active?.id != before?.id || active?.token != before?.token) return;
    await ref.read(pcOperationsProvider.notifier).changeSessions(target, close);
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(connectionControllerProvider, (previous, next) {
      if (previous?.value?.activeId != next.value?.activeId) {
        setState(() => _closeExisting = false);
      }
    });
    final targets = ref.watch(sessionChangeTargetsProvider(_target));
    final operation = ref.watch(pcOperationsProvider);
    final busy =
        operation.isLoading ||
        ref.watch(pcSettingsProvider).value?.isSaving == true;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(AppSizes.p16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              context.tr('기존 세션 일괄 변경'),
              style: Theme.of(context).textTheme.titleMedium,
            ),
            const SizedBox(height: AppSizes.p8),
            Text(context.tr('새 태스크의 기본값과 별개로, 기존 태스크의 세션 종류를 바꿉니다.')),
            const SizedBox(height: AppSizes.p16),
            Semantics(
              identifier: 'bulk-session-type',
              child: DropdownButtonFormField<SessionType>(
                initialValue: _target,
                decoration: InputDecoration(labelText: context.tr('변경할 세션 종류')),
                items: [
                  for (final type in SessionType.values)
                    DropdownMenuItem(value: type, child: Text(type.wireValue)),
                ],
                onChanged: busy
                    ? null
                    : (value) {
                        if (value != null) setState(() => _target = value);
                      },
              ),
            ),
            const SizedBox(height: AppSizes.p8),
            targets.when(
              loading: () => const LinearProgressIndicator(),
              error: (error, _) => Column(
                children: [
                  Text(context.tr(pcRequestError(error))),
                  TextButton(
                    onPressed: () =>
                        ref.invalidate(sessionChangeTargetsProvider(_target)),
                    child: Text(context.tr('대상 다시 조회')),
                  ),
                ],
              ),
              data: (value) => Text(
                context.tr('변경 대상 {count}개 · 실행 중 이전 세션 {live}개', {
                  'count': value.taskCount,
                  'live': value.liveSessionCount,
                }),
              ),
            ),
            CheckboxListTile(
              contentPadding: EdgeInsets.zero,
              title: Text(context.tr('기존 세션 종료')),
              subtitle: Text(context.tr('선택하지 않으면 기존 프로세스는 PC에서 계속 실행됩니다.')),
              value: _closeExisting,
              onChanged: busy
                  ? null
                  : (value) => setState(() => _closeExisting = value ?? false),
            ),
            Semantics(
              identifier: 'bulk-session-apply',
              child: OutlinedButton(
                onPressed:
                    busy ||
                        targets.value == null ||
                        targets.requireValue.taskCount == 0
                    ? null
                    : () => _apply(targets.requireValue),
                child: Text(
                  context.tr(operation.isLoading ? '처리하는 중…' : '변경 내용 확인'),
                ),
              ),
            ),
            if (operation.value case BulkSessionChangeResult(
              :final changedCount,
              :final failedCount,
            ))
              Padding(
                padding: const EdgeInsets.only(top: AppSizes.p12),
                child: Text(
                  context.tr('{changed}개 변경 · {failed}개 실패', {
                    'changed': changedCount,
                    'failed': failedCount,
                  }),
                ),
              ),
          ],
        ),
      ),
    );
  }
}
