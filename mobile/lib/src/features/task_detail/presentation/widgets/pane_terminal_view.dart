import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:xterm/xterm.dart' as xterm;

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_theme.dart';
import '../../../../common/network/desktop_client.dart';
import '../../../../common/network/desktop_connection.dart';
import '../../../../common/providers.dart';
import '../../domain/mirror_pane.dart';
import 'terminal_soft_keyboard.dart';

/// 크기 조정이 가능한 세션은 모바일 viewport에 맞추고, 나머지는 원본 격자를 스크롤로 읽는다.
class PaneTerminalView extends ConsumerStatefulWidget {
  const PaneTerminalView({
    required this.taskId,
    required this.pane,
    this.canResize = false,
    super.key,
  });

  final String taskId;
  final MirrorPane pane;
  final bool canResize;

  @override
  ConsumerState<PaneTerminalView> createState() => _PaneTerminalViewState();
}

class _PaneTerminalViewState extends ConsumerState<PaneTerminalView>
    with WidgetsBindingObserver {
  late final xterm.Terminal _terminal;
  PaneStream? _stream;
  DesktopConnection? _connection;
  StreamSubscription<String>? _subscription;
  String? _failureMessage;
  bool _hasOutput = false;
  Timer? _firstFrameTimer;
  final _horizontalScroll = ScrollController();
  final _verticalScroll = ScrollController();
  ({int cols, int rows})? _lastSentSize;
  bool _isSuspended = false;
  int _streamGeneration = 0;

  static const _firstFrameTimeout = Duration(seconds: 15);
  static final _textStyle = xterm.TerminalStyle(
    fontSize: 14,
    fontFamily: monoFontFamily,
    fontFamilyFallback: [
      'SymbolsNerdFontMono',
      ...const xterm.TerminalStyle().fontFamilyFallback,
    ],
  );

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _terminal = xterm.Terminal(maxLines: _scrollbackLines);
    _terminal.resize(widget.pane.width, widget.pane.height);

    _terminal.onOutput = (data) {
      if (_failureMessage == null) _stream?.write(data);
    };
    _terminal.onResize = (cols, rows, _, _) => _sendViewport(cols, rows);

    unawaited(_openStream());
  }

  @override
  void didUpdateWidget(PaneTerminalView oldWidget) {
    super.didUpdateWidget(oldWidget);

    /// 원본 격자 모드에서 데스크탑 pane을 분할하거나 창 크기를 바꾸면 같은 pane id로 새 좌표가 온다.
    /// key가 pane id라 element가 재사용되므로, 여기서 다시 재지 않으면 상자만 커지고 격자는 옛 크기로 남는다.
    if (!widget.canResize &&
        (oldWidget.canResize ||
            widget.pane.width != oldWidget.pane.width ||
            widget.pane.height != oldWidget.pane.height)) {
      _terminal.resize(widget.pane.width, widget.pane.height);
    }
  }

  /// 데스크탑 터미널이 보관하는 양과 비슷하게 잡는다. 더 늘리면 폰 메모리만 먹는다
  static const _scrollbackLines = 2000;

  void _sendViewport(int cols, int rows) {
    final size = (cols: cols, rows: rows);
    if (!widget.canResize ||
        _stream == null ||
        _isSuspended ||
        _failureMessage != null ||
        size == _lastSentSize) {
      return;
    }
    _stream!.resize(cols, rows);
    _lastSentSize = size;
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed && _isSuspended) {
      _isSuspended = false;
      unawaited(_retryStream());
    } else if (state == AppLifecycleState.paused ||
        state == AppLifecycleState.hidden ||
        state == AppLifecycleState.detached) {
      _isSuspended = true;
      _streamGeneration += 1;
      _firstFrameTimer?.cancel();
      unawaited(_subscription?.cancel());
      unawaited(_stream?.close());
      _stream = null;
    }
  }

  Future<void> _openStream() async {
    final generation = ++_streamGeneration;
    _firstFrameTimer = Timer(_firstFrameTimeout, () {
      _failStream('터미널 응답이 없습니다. 데스크탑 연결을 확인한 뒤 다시 연결해 주세요.');
    });
    try {
      final connection = (await ref.read(connectionControllerProvider.future))
          .activeConnection;
      if (!mounted || _isSuspended || generation != _streamGeneration) return;
      if (connection == null) {
        _failStream('데스크탑에 연결되어 있지 않습니다. 다시 페어링해 주세요.');
        return;
      }
      final stream = ref
          .read(desktopClientProvider)
          .openPaneStream(connection, widget.taskId, widget.pane.id);
      _connection = connection;
      _stream = stream;
      _lastSentSize = null;
      _subscription = stream.output.listen(
        (data) {
          if (generation == _streamGeneration) _receiveOutput(data);
        },
        onError: (_) {
          if (generation == _streamGeneration) {
            _failStream('데스크탑과 연결이 끊겼습니다. 네트워크를 확인해 주세요.');
          }
        },
        onDone: () {
          if (generation == _streamGeneration) _handleStreamClosed();
        },
      );
      _sendViewport(_terminal.viewWidth, _terminal.viewHeight);
    } catch (_) {
      if (generation == _streamGeneration) {
        _failStream('데스크탑에 연결하지 못했습니다. 주소와 네트워크를 확인해 주세요.');
      }
    }
  }

  void _receiveOutput(String data) {
    if (!mounted || _failureMessage != null || data.isEmpty) return;
    _firstFrameTimer?.cancel();
    _terminal.write(data);
    if (!_hasOutput) setState(() => _hasOutput = true);
  }

  Future<void> _retryStream() async {
    _firstFrameTimer?.cancel();
    unawaited(_subscription?.cancel());
    unawaited(_stream?.close());
    if (!mounted) return;
    _stream = null;
    setState(() {
      _failureMessage = null;
      _hasOutput = false;
    });
    await _openStream();
  }

  /// 데스크탑이 이 기기를 끊었다면 다시 페어링하는 것 말고 사용자가 할 일이 없다.
  ///
  /// 그것을 세션 문제로 접어 보여 주면 사용자는 데스크탑에서 세션을 여닫으며 원인을 찾게 되고,
  /// 실제로 해야 할 일은 화면 어디에도 나타나지 않는다. 서버가 종료 코드로 나눠 보낸 사유가 여기서 쓰인다.
  void _handleStreamClosed() {
    if (_stream?.readCloseCode() == deviceUnpairedCloseCode) {
      final connection = _connection;
      if (connection != null) {
        unawaited(
          ref
              .read(connectionControllerProvider.notifier)
              .forgetLocally(connection.id, connection.token),
        );
      }
      return;
    }

    if (_failureMessage != null) return;
    _failStream(
      _stream?.readCloseCode() == 4000
          ? '데스크탑의 터미널 스트림에 오류가 발생했습니다. 세션 상태와 데스크탑 로그를 확인해 주세요.'
          : '데스크탑과 연결이 끊겼습니다. 네트워크를 확인해 주세요.',
    );
  }

  void _failStream(String message) {
    _firstFrameTimer?.cancel();
    if (mounted) setState(() => _failureMessage = message);
  }

  @override
  void dispose() {
    /// 마지막 구독자가 나가면 데스크탑이 pane에 걸어 둔 파이프도 함께 풀린다
    _firstFrameTimer?.cancel();
    WidgetsBinding.instance.removeObserver(this);
    _streamGeneration += 1;
    _horizontalScroll.dispose();
    _verticalScroll.dispose();
    unawaited(_subscription?.cancel());
    unawaited(_stream?.close());
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final failureMessage = _failureMessage;
    if (failureMessage != null) {
      return _PaneStreamFailure(message: failureMessage, onRetry: _retryStream);
    }

    final metrics = TextPainter(
      text: TextSpan(text: 'mmmmmmmmmm', style: _textStyle.toTextStyle()),
      textDirection: TextDirection.ltr,
      textScaler: MediaQuery.textScalerOf(context),
    )..layout();
    final terminalWidth = (metrics.width / 10 * widget.pane.width)
        .ceilToDouble();
    metrics.dispose();

    return ColoredBox(
      color: AppColors.terminalBg,
      child: Column(
        children: [
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
            child: Row(
              children: [
                Icon(
                  Icons.circle,
                  size: 8,
                  color: _hasOutput
                      ? AppColors.statusDone
                      : AppColors.statusProgress,
                ),
                const SizedBox(width: 8),
                Text(
                  _hasOutput ? '출력 수신 중' : '터미널 연결 중…',
                  style: Theme.of(context).textTheme.bodySmall,
                ),
                const Spacer(),
                Text(
                  widget.canResize ? '화면 크기에 맞춤' : '좌우·위아래 스크롤',
                  style: Theme.of(context).textTheme.bodySmall,
                ),
              ],
            ),
          ),
          Expanded(
            child: LayoutBuilder(
              builder: (context, constraints) {
                return Scrollbar(
                  controller: _horizontalScroll,
                  thumbVisibility: true,
                  notificationPredicate: (notification) =>
                      notification.metrics.axis == Axis.horizontal,
                  child: SingleChildScrollView(
                    controller: _horizontalScroll,
                    scrollDirection: Axis.horizontal,
                    child: SizedBox(
                      width:
                          !widget.canResize &&
                              terminalWidth > constraints.maxWidth
                          ? terminalWidth
                          : constraints.maxWidth,
                      height: constraints.maxHeight,
                      child: MediaQuery.removePadding(
                        context: context,
                        removeLeft: true,
                        removeRight: true,
                        removeTop: true,
                        removeBottom: true,
                        child: TerminalSoftKeyboard(
                          terminal: _terminal,
                          child: Semantics(
                            identifier: 'pane-terminal',
                            label: '터미널 입력',
                            child: xterm.TerminalView(
                              _terminal,
                              autoResize: widget.canResize,
                              hardwareKeyboardOnly: true,
                              scrollController: _verticalScroll,
                              backgroundOpacity: 0,
                              theme: _terminalTheme,
                              textStyle: _textStyle,
                            ),
                          ),
                        ),
                      ),
                    ),
                  ),
                );
              },
            ),
          ),
        ],
      ),
    );
  }
}

const _terminalTheme = xterm.TerminalTheme(
  cursor: AppColors.brandPrimary,
  selection: AppColors.brandSubtle,
  foreground: AppColors.textPrimary,
  background: AppColors.terminalBg,
  black: Color(0xFF3B3F49),
  red: Color(0xFFFF7D73),
  green: Color(0xFF65D08A),
  yellow: Color(0xFFF6C35F),
  blue: Color(0xFF0064FF),
  magenta: Color(0xFF9B8CFF),
  cyan: Color(0xFF5EC8D8),
  white: Color(0xFFF3F4F7),
  brightBlack: Color(0xFF717680),
  brightRed: Color(0xFFFFAAA2),
  brightGreen: Color(0xFF9BE6B4),
  brightYellow: Color(0xFFFFD58A),
  brightBlue: Color(0xFF6EA8FF),
  brightMagenta: Color(0xFFC4B5FD),
  brightCyan: Color(0xFF9FE7F1),
  brightWhite: Color(0xFFFFFFFF),
  searchHitBackground: Color(0xFFF6C35F),
  searchHitBackgroundCurrent: Color(0xFF0064FF),
  searchHitForeground: Color(0xFF090A0D),
);

class _PaneStreamFailure extends StatelessWidget {
  const _PaneStreamFailure({required this.message, required this.onRetry});

  final String message;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) {
    return ColoredBox(
      color: AppColors.terminalBg,
      child: Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(
                message,
                textAlign: TextAlign.center,
                style: Theme.of(context).textTheme.bodyMedium,
              ),
              const SizedBox(height: 16),
              TextButton(onPressed: onRetry, child: const Text('다시 연결')),
            ],
          ),
        ),
      ),
    );
  }
}
