import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_sizes.dart';
import '../../../common/l10n/app_localizations.dart';
import '../../../common/providers.dart';
import '../../task_create/domain/session_type.dart';
import '../application/pc_settings_controller.dart';
import 'widgets/bulk_session_change_section.dart';
import 'widgets/pc_sync_section.dart';
import 'widgets/pc_request_failure.dart';

class PcSettingsScreen extends ConsumerStatefulWidget {
  const PcSettingsScreen({super.key});
  @override
  ConsumerState<PcSettingsScreen> createState() => _PcSettingsScreenState();
}

class _PcSettingsScreenState extends ConsumerState<PcSettingsScreen> {
  final _interval = TextEditingController();
  final _form = GlobalKey<FormState>();
  bool _dirty = false;
  @override
  void initState() {
    super.initState();
    ref.listenManual(pcSettingsProvider, (_, next) {
      final value = next.value;
      if (value != null && !value.isSaving && !_dirty) {
        _interval.text = value.settings.intervalMinutes.toString();
      }
    }, fireImmediately: true);
  }

  @override
  void dispose() {
    _interval.dispose();
    super.dispose();
  }

  Future<void> _saveInterval() async {
    if (!_form.currentState!.validate()) return;
    final saved = await ref
        .read(pcSettingsProvider.notifier)
        .save(intervalMinutes: int.parse(_interval.text));
    if (mounted && saved) setState(() => _dirty = false);
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(connectionControllerProvider, (previous, next) {
      if (previous?.value?.activeId != next.value?.activeId) {
        _dirty = false;
        _interval.clear();
      }
    });
    final settings = ref.watch(pcSettingsProvider);
    final isOperating = ref.watch(
      pcOperationsProvider.select((value) => value.isLoading),
    );
    final connection = ref
        .watch(connectionControllerProvider)
        .value
        ?.activeConnection;
    return Scaffold(
      appBar: AppBar(
        title: Text(context.tr('PC 설정')),
        actions: [
          IconButton(
            tooltip: context.tr('새로고침'),
            onPressed: () => ref.invalidate(pcSettingsProvider),
            icon: const Icon(Icons.refresh),
          ),
        ],
      ),
      body: SafeArea(
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 840),
            child: settings.when(
              loading: () => const Center(child: CircularProgressIndicator()),
              error: (error, _) => PcRequestFailure(
                error: error,
                onRetry: () => ref.invalidate(pcSettingsProvider),
              ),
              data: (value) {
                final busy = value.isSaving || isOperating;
                return ListView(
                  padding: const EdgeInsets.all(AppSizes.p16),
                  children: [
                    Text(
                      connection?.displayName ?? context.tr('연결된 PC'),
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                    const SizedBox(height: AppSizes.p8),
                    Text(context.tr('이 설정은 선택한 PC의 데스크탑과 모바일에 함께 적용됩니다.')),
                    const SizedBox(height: AppSizes.p16),
                    Card(
                      child: Padding(
                        padding: const EdgeInsets.all(AppSizes.p16),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            Semantics(
                              identifier: 'pc-default-session',
                              child: DropdownButtonFormField<SessionType>(
                                key: ValueKey(
                                  value.settings.defaultSessionType,
                                ),
                                initialValue: value.settings.defaultSessionType,
                                decoration: InputDecoration(
                                  labelText: context.tr('새 태스크 기본 세션'),
                                ),
                                items: [
                                  for (final type in SessionType.values)
                                    DropdownMenuItem(
                                      value: type,
                                      child: Text(type.wireValue),
                                    ),
                                ],
                                onChanged: busy
                                    ? null
                                    : (type) {
                                        if (type != null) {
                                          ref
                                              .read(pcSettingsProvider.notifier)
                                              .save(defaultSessionType: type);
                                        }
                                      },
                              ),
                            ),
                            const SizedBox(height: AppSizes.p16),
                            Semantics(
                              identifier: 'pc-background-sync',
                              child: SwitchListTile.adaptive(
                                contentPadding: EdgeInsets.zero,
                                title: Text(context.tr('백그라운드 동기화')),
                                value: value.settings.isSyncEnabled,
                                onChanged: busy
                                    ? null
                                    : (enabled) => ref
                                          .read(pcSettingsProvider.notifier)
                                          .save(isSyncEnabled: enabled),
                              ),
                            ),
                            Form(
                              key: _form,
                              child: Semantics(
                                identifier: 'pc-sync-interval',
                                child: TextFormField(
                                  controller: _interval,
                                  enabled: !busy,
                                  keyboardType: TextInputType.number,
                                  decoration: InputDecoration(
                                    labelText: context.tr('동기화 주기 (분)'),
                                    helperText: context.tr('1분에서 1440분 사이'),
                                  ),
                                  onChanged: (_) =>
                                      setState(() => _dirty = true),
                                  validator: (value) {
                                    final minutes = int.tryParse(value ?? '');
                                    return minutes != null &&
                                            minutes >= 1 &&
                                            minutes <= 1440
                                        ? null
                                        : context.tr('1분에서 1440분 사이로 입력해 주세요.');
                                  },
                                ),
                              ),
                            ),
                            const SizedBox(height: AppSizes.p12),
                            FilledButton(
                              onPressed: busy || !_dirty ? null : _saveInterval,
                              child: Text(
                                context.tr(
                                  value.isSaving ? '저장하는 중…' : '주기 저장',
                                ),
                              ),
                            ),
                            if (value.error != null)
                              Padding(
                                padding: const EdgeInsets.only(
                                  top: AppSizes.p8,
                                ),
                                child: Text(
                                  context.tr(pcRequestError(value.error)),
                                  style: TextStyle(
                                    color: Theme.of(context).colorScheme.error,
                                  ),
                                ),
                              ),
                          ],
                        ),
                      ),
                    ),
                    const SizedBox(height: AppSizes.p16),
                    const BulkSessionChangeSection(),
                    const SizedBox(height: AppSizes.p16),
                    const PcSyncSection(),
                  ],
                );
              },
            ),
          ),
        ),
      ),
    );
  }
}
