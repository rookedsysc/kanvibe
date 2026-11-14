import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_sizes.dart';
import '../../../common/l10n/app_localizations.dart';
import '../application/ai_accounts_controller.dart';
import '../domain/ai_usage.dart';
import 'ai_accounts_screen.dart';
import 'widgets/ai_request_failure.dart';

class AiUsageScreen extends ConsumerWidget {
  const AiUsageScreen({super.key});
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final usage = ref.watch(aiUsageProvider);
    return Scaffold(
      appBar: AppBar(
        title: Text(context.tr('AI 사용량')),
        actions: [
          IconButton(
            tooltip: context.tr('AI 계정 관리'),
            onPressed: () => Navigator.of(context).push(
              MaterialPageRoute<void>(builder: (_) => const AiAccountsScreen()),
            ),
            icon: const Icon(Icons.manage_accounts_outlined),
          ),
          IconButton(
            tooltip: context.tr('사용량 새로고침'),
            onPressed: () => ref.invalidate(aiUsageProvider),
            icon: const Icon(Icons.refresh),
          ),
        ],
      ),
      body: SafeArea(
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 840),
            child: usage.when(
              loading: () => const Center(child: CircularProgressIndicator()),
              error: (error, _) => AiRequestFailure(
                error: error,
                onRetry: () => ref.invalidate(aiUsageProvider),
              ),
              data: (snapshot) => RefreshIndicator(
                onRefresh: () async {
                  await AsyncValue.guard(
                    () => ref.refresh(aiUsageProvider.future),
                  );
                },
                child: ListView(
                  padding: const EdgeInsets.all(AppSizes.p16),
                  physics: const AlwaysScrollableScrollPhysics(),
                  children: [
                    if (snapshot.fetchedAt != null)
                      Text(
                        context.tr('조회 {time}', {
                          'time': snapshot.fetchedAt!
                              .toLocal()
                              .toString()
                              .split('.')
                              .first,
                        }),
                        style: Theme.of(context).textTheme.bodySmall,
                      ),
                    if (snapshot.accounts.isEmpty)
                      Padding(
                        padding: EdgeInsets.all(AppSizes.p24),
                        child: Text(
                          context.tr('사용량을 조회할 계정이 없습니다. AI 계정에서 로그인해 주세요.'),
                        ),
                      ),
                    for (final account in snapshot.accounts)
                      _UsageAccountCard(account: account),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _UsageAccountCard extends StatelessWidget {
  const _UsageAccountCard({required this.account});
  final AiUsageAccount account;
  @override
  Widget build(BuildContext context) => Card(
    margin: const EdgeInsets.only(top: AppSizes.p12),
    child: Padding(
      padding: const EdgeInsets.all(AppSizes.p16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(account.label, style: Theme.of(context).textTheme.titleMedium),
          Text(
            '${account.provider}${account.planName == null ? '' : ' · ${account.planName}'}',
          ),
          if (account.reason != null)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: AppSizes.p12),
              child: Text(
                context.tr(_reason(account.reason)),
                style: TextStyle(color: Theme.of(context).colorScheme.error),
              ),
            ),
          if (account.windows.isEmpty)
            Padding(
              padding: EdgeInsets.only(top: AppSizes.p12),
              child: Text(context.tr('표시할 사용량 한도가 없습니다.')),
            ),
          if (account.status != 'ok' && account.windows.isNotEmpty)
            Text(context.tr('마지막으로 확인한 값입니다.')),
          for (final window in account.windows)
            Padding(
              padding: const EdgeInsets.only(top: AppSizes.p16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    context.tr('{period} · {percent}% 사용', {
                      'period': context.tr(_period(window)),
                      'percent': window.usedPercent.toStringAsFixed(1),
                    }),
                  ),
                  const SizedBox(height: AppSizes.p8),
                  Semantics(
                    label: context.tr('{period} {percent}퍼센트 사용', {
                      'period': context.tr(_period(window)),
                      'percent': window.usedPercent.toStringAsFixed(1),
                    }),
                    child: LinearProgressIndicator(
                      value: (window.usedPercent / 100).clamp(0.0, 1.0),
                      minHeight: AppSizes.p8,
                    ),
                  ),
                  if (window.resetsAt != null)
                    Padding(
                      padding: const EdgeInsets.only(top: AppSizes.p4),
                      child: Text(
                        context.tr('초기화 {time}', {
                          'time': window.resetsAt!
                              .toLocal()
                              .toString()
                              .split('.')
                              .first,
                        }),
                        style: Theme.of(context).textTheme.bodySmall,
                      ),
                    ),
                ],
              ),
            ),
          if (account.fetchedAt != null)
            Padding(
              padding: const EdgeInsets.only(top: AppSizes.p16),
              child: Text(
                context.tr('계정 조회 {time}', {
                  'time': account.fetchedAt!
                      .toLocal()
                      .toString()
                      .split('.')
                      .first,
                }),
                style: Theme.of(context).textTheme.bodySmall,
              ),
            ),
        ],
      ),
    ),
  );
}

String _period(AiUsageWindow window) => switch (window.kind) {
  'session' => '세션 한도',
  'weekly' => '주간 한도',
  'monthly' => '월간 한도',
  'model' => window.modelName ?? '모델 한도',
  _ => window.kind,
};
String _reason(String? reason) => switch (reason) {
  'missing-credentials' => '로그인이 필요합니다.',
  'expired-credentials' => '로그인이 만료되었습니다. 다시 로그인해 주세요.',
  'keychain-unreadable' => 'PC의 키체인 접근을 확인해 주세요.',
  'gemini-cli-not-found' => 'PC에 Gemini CLI를 설치해 주세요.',
  'rate-limited' => '잠시 요청이 제한되었습니다. 잠시 뒤 새로고침해 주세요.',
  'empty-response' => 'AI 제공자가 사용량을 반환하지 않았습니다.',
  _ => '사용량을 조회하지 못했습니다. 다시 시도해 주세요.',
};
