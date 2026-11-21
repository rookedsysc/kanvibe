import '../../../common/l10n/app_localizations.dart';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../../../common/constants/app_theme.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';

/// 데스크탑을 처음 연결하거나 목록에 새 PC를 추가하는 화면.
class ConnectionScreen extends ConsumerStatefulWidget {
  const ConnectionScreen({this.isAdding = false, super.key});

  final bool isAdding;

  @override
  ConsumerState<ConnectionScreen> createState() => _ConnectionScreenState();
}

class _ConnectionScreenState extends ConsumerState<ConnectionScreen> {
  final _formKey = GlobalKey<FormState>();
  final _hostController = TextEditingController();
  final _nameController = TextEditingController();
  final _portController = TextEditingController(
    text: '${DesktopConnection.defaultPort}',
  );
  final _codeController = TextEditingController();

  bool _isConnecting = false;
  String? _failureMessage;

  @override
  void dispose() {
    _hostController.dispose();
    _nameController.dispose();
    _portController.dispose();
    _codeController.dispose();
    super.dispose();
  }

  Future<void> _connect() async {
    if (!_formKey.currentState!.validate()) {
      return;
    }

    setState(() {
      _isConnecting = true;
      _failureMessage = null;
    });

    ConnectFailure? failureReason;
    try {
      failureReason = await ref
          .read(connectionControllerProvider.notifier)
          .connect(
            host: _hostController.text.trim(),
            port: int.parse(_portController.text.trim()),
            code: _codeController.text.trim(),
            name: _nameController.text.trim(),
          );
    } catch (_) {
      if (mounted) {
        setState(() {
          _isConnecting = false;
          _failureMessage = '데스크탑에 연결하지 못했습니다. 주소와 네트워크를 확인해 주세요.';
        });
      }
      return;
    }

    if (!mounted) {
      return;
    }
    if (failureReason == null && widget.isAdding) {
      Navigator.of(context).popUntil((route) => route.isFirst);
      return;
    }
    setState(() {
      _isConnecting = false;
      _failureMessage = failureReason == null
          ? null
          : _connectFailureMessage(failureReason);
    });
  }

  String _connectFailureMessage(ConnectFailure reason) => switch (reason) {
    ConnectFailure.rejectedCode => '코드가 맞지 않거나 만료되었습니다.',
    ConnectFailure.alreadyPaired => '이미 등록된 PC입니다. 목록에서 선택해 주세요.',
  };

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: widget.isAdding ? AppBar(title: Text(context.tr('PC 추가'))) : null,
      body: SafeArea(
        child: Center(
          child: SingleChildScrollView(
            padding: EdgeInsets.all(AppSizes.p24),
            child: ConstrainedBox(
              constraints: BoxConstraints(maxWidth: 420),
              child: Form(
                key: _formKey,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Align(
                      alignment: Alignment.centerLeft,
                      child: Image.asset(
                        'assets/branding/kanvibe-logo.png',
                        width: 180,
                        color: context.colors.textPrimary,
                        semanticLabel: 'KanVibe',
                      ),
                    ),
                    SizedBox(height: AppSizes.p32),
                    Text(
                      context.tr(widget.isAdding ? '새 PC 연결' : '데스크탑에 연결'),
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                    SizedBox(height: AppSizes.p8),
                    Text(
                      context.tr(
                        'PC의 KanVibe 설정에서 연결 코드를 만든 다음 입력해 주세요. 등록한 PC는 목록에서 다시 선택할 수 있습니다.',
                      ),
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                    SizedBox(height: AppSizes.p24),
                    TextFormField(
                      controller: _nameController,
                      decoration: InputDecoration(
                        labelText: context.tr('PC 이름 (선택)'),
                        helperText: context.tr('비워 두면 주소가 표시됩니다'),
                      ),
                      textInputAction: TextInputAction.next,
                    ),
                    SizedBox(height: AppSizes.p16),
                    TextFormField(
                      controller: _hostController,
                      decoration: InputDecoration(
                        labelText: context.tr('데스크탑 주소'),
                        helperText: context.tr('같은 네트워크에서 보이는 IP 주소'),
                      ),
                      keyboardType: TextInputType.url,
                      autocorrect: false,
                      style: monoStyle(
                        fontSize: 14,
                        color: context.colors.textPrimary,
                      ),
                      validator: (value) => (value ?? '').trim().isEmpty
                          ? context.tr('주소를 입력해 주세요')
                          : null,
                    ),
                    SizedBox(height: AppSizes.p16),
                    TextFormField(
                      controller: _portController,
                      decoration: InputDecoration(labelText: context.tr('포트')),
                      keyboardType: TextInputType.number,
                      inputFormatters: [FilteringTextInputFormatter.digitsOnly],
                      style: monoStyle(
                        fontSize: 14,
                        color: context.colors.textPrimary,
                      ),
                      validator: (value) {
                        final port = int.tryParse((value ?? '').trim());
                        return port == null || port < 1 || port > 65535
                            ? context.tr('1~65535 사이의 포트를 입력해 주세요')
                            : null;
                      },
                    ),
                    SizedBox(height: AppSizes.p16),
                    TextFormField(
                      controller: _codeController,
                      decoration: InputDecoration(
                        labelText: context.tr('연결 코드'),
                        helperText: context.tr('데스크탑 화면에 뜬 6자리 숫자'),
                      ),
                      keyboardType: TextInputType.number,
                      inputFormatters: [
                        FilteringTextInputFormatter.digitsOnly,
                        LengthLimitingTextInputFormatter(6),
                      ],
                      style: monoStyle(
                        fontSize: 20,
                        color: context.colors.textPrimary,
                      ),
                      validator: (value) => (value ?? '').trim().length != 6
                          ? context.tr('6자리를 입력해 주세요')
                          : null,
                    ),
                    if (_failureMessage != null) ...[
                      SizedBox(height: AppSizes.p16),
                      Text(
                        context.tr(_failureMessage!),
                        style: Theme.of(context).textTheme.bodySmall
                            ?.copyWith(color: AppColors.statusError),
                      ),
                    ],
                    SizedBox(height: AppSizes.p24),
                    FilledButton(
                      onPressed: _isConnecting ? null : _connect,
                      child: _isConnecting
                          ? SizedBox(
                              width: 20,
                              height: 20,
                              child: CircularProgressIndicator(strokeWidth: 2),
                            )
                          : Text(context.tr('연결')),
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
