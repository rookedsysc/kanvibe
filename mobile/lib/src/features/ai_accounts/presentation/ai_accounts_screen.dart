import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_sizes.dart';
import '../../../common/l10n/app_localizations.dart';
import '../../../common/providers.dart';
import '../application/ai_accounts_controller.dart';
import '../domain/ai_account.dart';
import 'ai_account_login_screen.dart';
import 'widgets/ai_account_form.dart';
import 'widgets/ai_request_failure.dart';

class AiAccountsScreen extends ConsumerWidget {
  const AiAccountsScreen({super.key});
  void _openLogin(BuildContext context, WidgetRef ref, AiAccount account) {
    final connection = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    if (connection == null) return;
    Navigator.of(context)
        .push(
          MaterialPageRoute<void>(
            builder: (_) =>
                AiAccountLoginScreen(account: account, connection: connection),
          ),
        )
        .whenComplete(() {
          if (context.mounted) {
            ref.invalidate(aiAccountsProvider);
            ref.invalidate(aiUsageProvider);
          }
        });
  }

  Future<void> _add(BuildContext context, WidgetRef ref) async {
    final account = await showDialog<AiAccount>(
      context: context,
      builder: (_) => const AiAccountForm(),
    );
    if (context.mounted && account != null) _openLogin(context, ref, account);
  }

  Future<void> _remove(
    BuildContext context,
    WidgetRef ref,
    AiAccount account,
  ) async {
    final original = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(context.tr('{name} 계정을 삭제할까요?', {'name': account.label})),
        content: Text(
          context.tr('PC의 CLI에서 로그아웃하고 KanVibe가 만든 계정 설정 폴더를 삭제합니다.'),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(context.tr('취소')),
          ),
          TextButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(context.tr('삭제')),
          ),
        ],
      ),
    );
    if (!context.mounted || confirmed != true) return;
    final active = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    if (active?.id != original?.id || active?.token != original?.token) return;
    await ref.read(aiAccountActionsProvider.notifier).remove(account.accountId);
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final accounts = ref.watch(aiAccountsProvider);
    final action = ref.watch(aiAccountActionsProvider);
    final connection = ref
        .watch(connectionControllerProvider)
        .value
        ?.activeConnection;
    return Scaffold(
      appBar: AppBar(
        title: Text(context.tr('AI 계정')),
        actions: [
          IconButton(
            tooltip: context.tr('새로고침'),
            onPressed: () => ref.invalidate(aiAccountsProvider),
            icon: const Icon(Icons.refresh),
          ),
        ],
      ),
      floatingActionButton: Semantics(
        identifier: 'ai-add-account',
        child: FloatingActionButton.extended(
          onPressed: action.isLoading || connection == null
              ? null
              : () => _add(context, ref),
          icon: const Icon(Icons.add),
          label: Text(context.tr('계정 추가')),
        ),
      ),
      body: SafeArea(
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 840),
            child: accounts.when(
              loading: () => const Center(child: CircularProgressIndicator()),
              error: (error, _) => AiRequestFailure(
                error: error,
                onRetry: () => ref.invalidate(aiAccountsProvider),
              ),
              data: (values) => RefreshIndicator(
                onRefresh: () async {
                  await AsyncValue.guard(
                    () => ref.refresh(aiAccountsProvider.future),
                  );
                },
                child: ListView(
                  physics: const AlwaysScrollableScrollPhysics(),
                  padding: const EdgeInsets.fromLTRB(
                    AppSizes.p16,
                    AppSizes.p16,
                    AppSizes.p16,
                    AppSizes.p32 * 3,
                  ),
                  children: [
                    Text(
                      connection?.displayName ?? context.tr('연결된 PC'),
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                    const SizedBox(height: AppSizes.p8),
                    Text(
                      context.tr('이 PC에서 사용하는 CLI 계정입니다. 로그인 정보는 PC에 보관됩니다.'),
                    ),
                    if (action.hasError)
                      Padding(
                        padding: const EdgeInsets.symmetric(
                          vertical: AppSizes.p12,
                        ),
                        child: Text(
                          context.tr(aiRequestErrorMessage(action.error)),
                          style: TextStyle(
                            color: Theme.of(context).colorScheme.error,
                          ),
                        ),
                      ),
                    if (values.isEmpty)
                      Padding(
                        padding: EdgeInsets.all(AppSizes.p24),
                        child: Text(
                          context.tr('등록된 AI 계정이 없습니다. 계정을 추가해 주세요.'),
                        ),
                      ),
                    for (final account in values)
                      Card(
                        margin: const EdgeInsets.only(top: AppSizes.p12),
                        child: Padding(
                          padding: const EdgeInsets.all(AppSizes.p16),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Text(
                                account.label,
                                style: Theme.of(context).textTheme.titleMedium,
                              ),
                              Text(
                                '${account.provider.label} · ${account.accountName ?? context.tr('기본 계정')}',
                              ),
                              const SizedBox(height: AppSizes.p8),
                              Wrap(
                                spacing: AppSizes.p8,
                                children: [
                                  Chip(
                                    avatar: Icon(
                                      account.isLoggedIn
                                          ? Icons.check_circle_outline
                                          : Icons.login,
                                      size: AppSizes.p16,
                                    ),
                                    label: Text(
                                      account.isLoggedIn
                                          ? context.tr('로그인됨')
                                          : context.tr('로그인 필요'),
                                    ),
                                  ),
                                  if (account.planName != null)
                                    Chip(label: Text(account.planName!)),
                                ],
                              ),
                              Wrap(
                                spacing: AppSizes.p12,
                                children: [
                                  Semantics(
                                    identifier: 'ai-account-login',
                                    child: TextButton.icon(
                                      onPressed: action.isLoading
                                          ? null
                                          : () => _openLogin(
                                              context,
                                              ref,
                                              account,
                                            ),
                                      icon: const Icon(Icons.terminal),
                                      label: Text(
                                        account.isLoggedIn
                                            ? context.tr('다시 로그인')
                                            : context.tr('로그인'),
                                      ),
                                    ),
                                  ),
                                  if (account.isRemovable)
                                    Semantics(
                                      identifier: 'ai-account-delete',
                                      child: TextButton.icon(
                                        onPressed: action.isLoading
                                            ? null
                                            : () => _remove(
                                                context,
                                                ref,
                                                account,
                                              ),
                                        icon: const Icon(Icons.delete_outline),
                                        label: Text(context.tr('계정 삭제')),
                                      ),
                                    ),
                                ],
                              ),
                            ],
                          ),
                        ),
                      ),
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
