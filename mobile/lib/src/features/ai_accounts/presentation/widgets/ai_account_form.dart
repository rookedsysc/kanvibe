import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/constants/app_sizes.dart';
import '../../../../common/l10n/app_localizations.dart';
import '../../application/ai_accounts_controller.dart';
import '../../domain/ai_account.dart';
import 'ai_request_failure.dart';

class AiAccountForm extends ConsumerStatefulWidget {
  const AiAccountForm({super.key});
  @override
  ConsumerState<AiAccountForm> createState() => _AiAccountFormState();
}

class _AiAccountFormState extends ConsumerState<AiAccountForm> {
  final _form = GlobalKey<FormState>();
  final _name = TextEditingController();
  AiAccountProvider _provider = AiAccountProvider.claude;
  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    if (!_form.currentState!.validate()) return;
    final name = _name.text.trim();
    final provider = _provider;
    final id = await ref
        .read(aiAccountActionsProvider.notifier)
        .add(provider, name);
    if (mounted && id != null) {
      Navigator.of(context).pop(
        AiAccount(
          accountId: id,
          provider: provider,
          label: name,
          accountName: name,
          isLoggedIn: false,
          isRemovable: true,
        ),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    final action = ref.watch(aiAccountActionsProvider);
    return AlertDialog(
      title: Text(context.tr('AI 계정 추가')),
      content: SingleChildScrollView(
        child: Form(
          key: _form,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              DropdownButtonFormField<AiAccountProvider>(
                initialValue: _provider,
                decoration: InputDecoration(labelText: context.tr('AI 제공자')),
                items: [
                  for (final provider in AiAccountProvider.values)
                    DropdownMenuItem(
                      value: provider,
                      child: Text(provider.label),
                    ),
                ],
                onChanged: action.isLoading
                    ? null
                    : (value) {
                        if (value != null) setState(() => _provider = value);
                      },
              ),
              const SizedBox(height: AppSizes.p16),
              TextFormField(
                controller: _name,
                enabled: !action.isLoading,
                decoration: InputDecoration(
                  labelText: context.tr('계정 이름'),
                  helperText: context.tr('영문·숫자·하이픈·밑줄, 최대 40자'),
                ),
                maxLength: 40,
                autofocus: true,
                textInputAction: TextInputAction.done,
                onFieldSubmitted: (_) {
                  if (!action.isLoading) _submit();
                },
                validator: (value) =>
                    RegExp(r'^[A-Za-z0-9_-][A-Za-z0-9._-]{0,39}$')
                        .hasMatch(value?.trim() ?? '')
                    ? null
                    : context.tr('이름을 확인해 주세요. 경로는 입력할 수 없습니다.'),
              ),
              const SizedBox(height: AppSizes.p8),
              Text(context.tr('PC에 계정 설정을 추가한 뒤 CLI 로그인 터미널을 엽니다.')),
              if (action.hasError)
                Padding(
                  padding: const EdgeInsets.only(top: AppSizes.p12),
                  child: Text(
                    context.tr(aiRequestErrorMessage(action.error)),
                    style: TextStyle(
                      color: Theme.of(context).colorScheme.error,
                    ),
                  ),
                ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: action.isLoading
              ? null
              : () => Navigator.of(context).pop(),
          child: Text(context.tr('취소')),
        ),
        FilledButton(
          onPressed: action.isLoading ? null : _submit,
          child: Text(
            action.isLoading ? context.tr('추가하는 중…') : context.tr('추가하고 로그인'),
          ),
        ),
      ],
    );
  }
}
