import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../../../common/constants/app_theme.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';

/// 데스크탑에 처음 연결하는 화면.
///
/// 한 번만 지나가는 화면이다. 연결하고 나면 토큰이 기기에 남아 다음부터는 곧바로 보드가 열린다.
class ConnectionScreen extends ConsumerStatefulWidget {
  const ConnectionScreen({super.key});

  @override
  ConsumerState<ConnectionScreen> createState() => _ConnectionScreenState();
}

class _ConnectionScreenState extends ConsumerState<ConnectionScreen> {
  final _formKey = GlobalKey<FormState>();
  final _hostController = TextEditingController();
  final _portController = TextEditingController(
    text: '${DesktopConnection.defaultPort}',
  );
  final _codeController = TextEditingController();

  bool _isConnecting = false;
  String? _failureMessage;

  @override
  void dispose() {
    _hostController.dispose();
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

    final failure = await ref
        .read(connectionControllerProvider.notifier)
        .connect(
          host: _hostController.text.trim(),
          port: int.parse(_portController.text.trim()),
          code: _codeController.text.trim(),
        );

    if (!mounted) {
      return;
    }
    setState(() {
      _isConnecting = false;
      _failureMessage = failure;
    });
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: SafeArea(
        child: Center(
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(AppSizes.p24),
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 420),
              child: Form(
                key: _formKey,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Text(
                      '데스크탑에 연결',
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                    const SizedBox(height: AppSizes.p8),
                    Text(
                      'KanVibe 설정에서 연결 코드를 띄운 다음, 여기에 옮겨 적어 주세요. 한 번 연결하면 다음부터는 바로 열립니다.',
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                    const SizedBox(height: AppSizes.p24),
                    TextFormField(
                      controller: _hostController,
                      decoration: const InputDecoration(
                        labelText: '데스크탑 주소',
                        helperText: '같은 네트워크에서 보이는 IP 주소',
                      ),
                      keyboardType: TextInputType.url,
                      autocorrect: false,
                      style: monoStyle(
                        fontSize: 14,
                        color: AppColors.textPrimary,
                      ),
                      validator: (value) =>
                          (value ?? '').trim().isEmpty ? '주소를 입력해 주세요' : null,
                    ),
                    const SizedBox(height: AppSizes.p16),
                    TextFormField(
                      controller: _portController,
                      decoration: const InputDecoration(labelText: '포트'),
                      keyboardType: TextInputType.number,
                      inputFormatters: [FilteringTextInputFormatter.digitsOnly],
                      style: monoStyle(
                        fontSize: 14,
                        color: AppColors.textPrimary,
                      ),
                      validator: (value) =>
                          int.tryParse((value ?? '').trim()) == null
                          ? '숫자로 입력해 주세요'
                          : null,
                    ),
                    const SizedBox(height: AppSizes.p16),
                    TextFormField(
                      controller: _codeController,
                      decoration: const InputDecoration(
                        labelText: '연결 코드',
                        helperText: '데스크탑 화면에 뜬 6자리 숫자',
                      ),
                      keyboardType: TextInputType.number,
                      inputFormatters: [
                        FilteringTextInputFormatter.digitsOnly,
                        LengthLimitingTextInputFormatter(6),
                      ],
                      style: monoStyle(
                        fontSize: 20,
                        color: AppColors.textPrimary,
                      ),
                      validator: (value) => (value ?? '').trim().length != 6
                          ? '6자리를 입력해 주세요'
                          : null,
                    ),
                    if (_failureMessage != null) ...[
                      const SizedBox(height: AppSizes.p16),
                      Text(
                        _failureMessage!,
                        style: Theme.of(context).textTheme.bodySmall
                            ?.copyWith(color: AppColors.statusError),
                      ),
                    ],
                    const SizedBox(height: AppSizes.p24),
                    FilledButton(
                      onPressed: _isConnecting ? null : _connect,
                      child: _isConnecting
                          ? const SizedBox(
                              width: 20,
                              height: 20,
                              child: CircularProgressIndicator(strokeWidth: 2),
                            )
                          : const Text('연결'),
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
