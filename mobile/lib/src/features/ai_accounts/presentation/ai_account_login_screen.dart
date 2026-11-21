import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';
import 'package:xterm/xterm.dart' as xterm;

import '../../../common/constants/app_sizes.dart';
import '../../../common/l10n/app_localizations.dart';
import '../../../common/constants/app_theme.dart';
import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../../task_detail/presentation/widgets/terminal_soft_keyboard.dart';
import '../application/ai_accounts_controller.dart';
import '../data/ai_login_frame.dart';
import '../domain/ai_account.dart';

class AiAccountLoginScreen extends ConsumerStatefulWidget {
  const AiAccountLoginScreen({
    required this.account,
    required this.connection,
    super.key,
  });
  final AiAccount account;
  final DesktopConnection connection;
  @override
  ConsumerState<AiAccountLoginScreen> createState() =>
      _AiAccountLoginScreenState();
}

class _AiAccountLoginScreenState extends ConsumerState<AiAccountLoginScreen>
    with WidgetsBindingObserver {
  final _terminal = xterm.Terminal(maxLines: 2000);
  final _selection = xterm.TerminalController();
  final _verification = TextEditingController();
  PaneStream? _stream;
  StreamSubscription<String>? _subscription;
  Timer? _deadline;
  int _generation = 0;
  bool _ready = false;
  bool _pcChanged = false;
  bool _exited = false;
  String? _notice;
  Map<String, Object> _noticeArguments = const {};
  String _urlTail = '';
  final _links = <String>{};
  static const _connectTimeout = Duration(seconds: 15);
  static const _maxUrlScanCharacters = 8192;
  static const _maxVisibleUrls = 3;
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _terminal.onOutput = (data) {
      if (_ready) _stream?.write(data);
    };
    _terminal.onResize = (cols, rows, _, _) {
      if (_ready) _stream?.resize(cols.clamp(2, 500), rows.clamp(1, 250));
    };
    unawaited(_connect());
  }

  void _disconnect() {
    _generation++;
    _deadline?.cancel();
    unawaited(_subscription?.cancel());
    unawaited(_stream?.close());
    _stream = null;
    _ready = false;
  }

  Future<void> _connect() async {
    _disconnect();
    final generation = _generation;
    if (_pcChanged) return;
    if (mounted) {
      setState(() {
        _notice = null;
        _exited = false;
        _links.clear();
        _urlTail = '';
      });
    }
    try {
      await ref.read(aiAccountActionsProvider.future);
      if (!mounted || generation != _generation) return;
      final stream = ref
          .read(aiAccountActionsProvider.notifier)
          .openLogin(widget.connection, widget.account.accountId);
      _stream = stream;
      _deadline = Timer(_connectTimeout, () {
        if (mounted && generation == _generation) {
          _fail('로그인 터미널 응답이 없습니다. PC 연결을 확인해 주세요.');
        }
      });
      _subscription = stream.output.listen(
        (message) {
          if (mounted && generation == _generation) _receive(message);
        },
        onError: (_) {
          if (mounted && generation == _generation) _fail('로그인 터미널 연결이 끊겼습니다.');
        },
        onDone: () {
          if (mounted && generation == _generation) _closed(stream);
        },
      );
    } catch (_) {
      if (mounted && generation == _generation) {
        _fail('로그인 터미널을 열지 못했습니다. PC 연결과 CLI 설치를 확인해 주세요.');
      }
    }
  }

  void _receive(String message) {
    try {
      final frame = AiLoginFrame.parse(message);
      switch (frame.type) {
        case AiLoginFrameType.ready:
          _deadline?.cancel();
          setState(() => _ready = true);
          _stream?.resize(
            _terminal.viewWidth.clamp(2, 500),
            _terminal.viewHeight.clamp(1, 250),
          );
        case AiLoginFrameType.output:
          _terminal.write(frame.data!);
          _findLinks(frame.data!);
        case AiLoginFrameType.exit:
          _deadline?.cancel();
          setState(() {
            _exited = true;
            _ready = false;
            _notice = '로그인 명령이 종료되었습니다 ({code}). 계정 목록에서 로그인 상태를 확인하세요.';
            _noticeArguments = {'code': frame.exitCode ?? -1};
          });
          ref.invalidate(aiAccountsProvider);
          ref.invalidate(aiUsageProvider);
        case AiLoginFrameType.error:
          _fail(frame.error!);
      }
    } on FormatException {
      _fail('데스크탑 로그인 응답을 읽지 못했습니다. 앱 버전을 확인해 주세요.');
    }
  }

  void _findLinks(String output) {
    final combined = '$_urlTail$output';
    _urlTail = combined.length > _maxUrlScanCharacters
        ? combined.substring(combined.length - _maxUrlScanCharacters)
        : combined;
    final found = RegExp(r'https?://[^\s\x1b<>()]+')
        .allMatches(_urlTail)
        .where((match) => match.end < _urlTail.length)
        .map((match) => match.group(0)!.replaceFirst(RegExp(r'[.,;]+$'), ''));
    var changed = false;
    for (final url in found) {
      if (_links.length < _maxVisibleUrls && _links.add(url)) changed = true;
    }
    if (changed) setState(() {});
  }

  void _fail(String message) {
    _disconnect();
    if (mounted) {
      setState(() {
        _notice = message;
        _noticeArguments = const {};
      });
    }
  }

  void _closed(PaneStream stream) {
    _deadline?.cancel();
    if (stream.readCloseCode() == deviceUnpairedCloseCode) {
      unawaited(
        ref
            .read(connectionControllerProvider.notifier)
            .forgetLocally(widget.connection.id, widget.connection.token),
      );
      _fail('PC에서 연결이 해제되었습니다. 다시 페어링해 주세요.');
    } else if (!_exited) {
      _fail('로그인 터미널이 종료되었습니다. 다시 연결하거나 계정 상태를 확인해 주세요.');
    }
  }

  Future<void> _openUrl(String value) async {
    final uri = Uri.tryParse(value);
    var opened = false;
    if (uri != null && const {'https', 'http'}.contains(uri.scheme)) {
      try {
        opened = await launchUrl(uri, mode: LaunchMode.externalApplication);
      } catch (_) {
        opened = false;
      }
    }
    if (mounted && !opened) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text(context.tr('로그인 링크를 열지 못했습니다. 링크를 복사해 브라우저에서 열어 주세요.')),
        ),
      );
    }
  }

  Future<void> _copy() async {
    try {
      await Clipboard.setData(
        ClipboardData(text: _terminal.buffer.getText(_selection.selection)),
      );
      if (mounted) {
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(SnackBar(content: Text(context.tr('터미널 출력을 복사했습니다.'))));
      }
    } on PlatformException {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text(context.tr('복사하지 못했습니다. 다시 시도해 주세요.'))),
        );
      }
    }
  }

  void _sendVerification() {
    if (!_ready || _verification.text.isEmpty) return;
    _stream?.write('${_verification.text}\r');
    _verification.clear();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    // 브라우저 OAuth 중에도 CLI가 응답을 기다려야 하므로 background 전환만으로 로그인 소켓을 닫지 않는다.
    if (state == AppLifecycleState.resumed) {
      ref.invalidate(aiAccountsProvider);
      ref.invalidate(aiUsageProvider);
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _disconnect();
    _selection.dispose();
    _verification.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(connectionControllerProvider, (_, next) {
      final active = next.value?.activeConnection;
      if (active?.id != widget.connection.id ||
          active?.token != widget.connection.token) {
        _pcChanged = true;
        _fail('선택한 PC가 변경되었습니다. 계정 목록에서 다시 로그인해 주세요.');
      }
    });
    final colors = Theme.of(context).colorScheme;
    return Scaffold(
      appBar: AppBar(
        title: Text(
          context.tr('{provider} 로그인', {
            'provider': widget.account.provider.label,
          }),
        ),
        actions: [
          IconButton(
            tooltip: context.tr('출력 복사'),
            onPressed: _copy,
            icon: const Icon(Icons.copy),
          ),
          IconButton(
            tooltip: context.tr('로그인 연결 종료'),
            onPressed: () {
              _disconnect();
              setState(() {
                _notice = '로그인 연결을 종료했습니다.';
                _noticeArguments = const {};
              });
            },
            icon: const Icon(Icons.stop_circle_outlined),
          ),
        ],
      ),
      body: SafeArea(
        child: Column(
          children: [
            Flexible(
              flex: 2,
              child: SingleChildScrollView(
                child: Padding(
                  padding: const EdgeInsets.all(AppSizes.p12),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        '${widget.connection.displayName} · ${widget.account.label}',
                        style: Theme.of(context).textTheme.titleSmall,
                      ),
                      const SizedBox(height: AppSizes.p4),
                      Text(
                        context.tr(
                          'CLI의 안내에 따라 로그인하세요. 링크는 브라우저에서 열고 인증 코드가 나오면 아래에 붙여 넣으세요.',
                        ),
                      ),
                      if (widget.account.provider == AiAccountProvider.gemini)
                        Padding(
                          padding: EdgeInsets.only(top: AppSizes.p8),
                          child: Text(
                            context.tr(
                              'Gemini는 브라우저가 PC의 CLI와 통신해야 하므로 PC에서 로그인해야 할 수 있습니다.',
                            ),
                          ),
                        ),
                      if (!_ready && _notice == null)
                        Padding(
                          padding: EdgeInsets.only(top: AppSizes.p8),
                          child: LinearProgressIndicator(),
                        ),
                      if (_notice != null)
                        Padding(
                          padding: const EdgeInsets.only(top: AppSizes.p8),
                          child: Text(
                            context.tr(_notice!, _noticeArguments),
                            style: TextStyle(
                              color: _exited ? colors.onSurface : colors.error,
                            ),
                          ),
                        ),
                      if (_notice != null && !_pcChanged)
                        TextButton(
                          onPressed: _connect,
                          child: Text(context.tr('다시 연결')),
                        ),
                      for (final link in _links)
                        Row(
                          children: [
                            Expanded(child: SelectableText(link, maxLines: 2)),
                            IconButton(
                              tooltip: context.tr('로그인 링크 열기'),
                              onPressed: () => _openUrl(link),
                              icon: const Icon(Icons.open_in_browser),
                            ),
                          ],
                        ),
                    ],
                  ),
                ),
              ),
            ),
            Expanded(
              flex: 3,
              child: Semantics(
                identifier: 'ai-login-terminal',
                label: context.tr('AI 로그인 터미널'),
                child: TerminalSoftKeyboard(
                  terminal: _terminal,
                  controller: _selection,
                  child: xterm.TerminalView(
                    _terminal,
                    controller: _selection,
                    hardwareKeyboardOnly: true,
                    textStyle: const xterm.TerminalStyle(
                      fontSize: 14,
                      fontFamily: monoFontFamily,
                    ),
                  ),
                ),
              ),
            ),
            Padding(
              padding: const EdgeInsets.all(AppSizes.p12),
              child: TextField(
                controller: _verification,
                enabled: _ready,
                decoration: InputDecoration(
                  labelText: context.tr('인증 코드 또는 응답'),
                  suffixIcon: IconButton(
                    tooltip: context.tr('인증 응답 전송'),
                    onPressed: _ready ? _sendVerification : null,
                    icon: const Icon(Icons.send),
                  ),
                ),
                onSubmitted: (_) => _sendVerification(),
                autocorrect: false,
                enableSuggestions: false,
                textInputAction: TextInputAction.send,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
