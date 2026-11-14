import '../../../../common/l10n/app_localizations.dart';

import 'dart:async';
import 'dart:convert';

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:xterm/xterm.dart' as xterm;

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_theme.dart';
import '../../../../common/network/desktop_client.dart';
import '../../../../common/network/desktop_connection.dart';
import '../../../../common/providers.dart';
import '../../domain/mirror_pane.dart';
import '../../../task_create/domain/session_type.dart';
import '../pane_chat_shortcut.dart';
import 'terminal_soft_keyboard.dart';

/// 선택한 pane을 화면 너비에 맞춰 표시한다.
class PaneTerminalView extends ConsumerStatefulWidget {
  const PaneTerminalView({
    required this.taskId,
    required this.pane,
    this.canResize = false,
    this.canRefreshHistory = false,
    this.canPrepareDirectConnection = true,
    this.sessionType = SessionType.terminal,
    this.canRespondToDeviceQueries = false,
    this.isActive = true,
    this.onToggleChat,
    super.key,
  });

  final SessionType sessionType;
  final String taskId;
  final MirrorPane pane;
  final bool canResize;
  final bool canRefreshHistory;
  final bool canPrepareDirectConnection;
  final bool canRespondToDeviceQueries;
  final bool isActive;
  final VoidCallback? onToggleChat;

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
  Timer? _firstFrameTimer;
  final _horizontalScroll = ScrollController();
  final _verticalScroll = ScrollController();
  final _terminalController = xterm.TerminalController();
  bool _isParsingRemoteOutput = false;
  String _pendingEscape = '';
  final _deferredOutput = <String>[];
  ({int cols, int rows})? _lastSentSize;
  bool _isSuspended = false;
  bool _isShortcutBusy = false;
  int _streamGeneration = 0;
  Completer<void>? _streamCancellation;
  Size? _cellSize;
  Offset? _touchedPosition;
  bool _hasRequestedHistoryForTouch = false;

  static const _screenRedrawPrefix = '\x1b[H\x1b[2J\x1b[3J';
  static const _firstFrameTimeout = Duration(seconds: 15);
  // 데스크톱 terminalClipboard.ts와 같은 base64 상한으로 원격 요청의 디코딩 비용을 제한한다.
  static const _maxClipboardPayloadLength = 1000000;
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
    _terminal = xterm.Terminal(
      maxLines: _scrollbackLines,
      mouseHandler: _TerminalWheelMouseHandler(_readTouchedCell),
    );
    _terminal.resize(widget.pane.width, widget.pane.height);
    _terminalController.addListener(_applyDeferredRedraw);

    _terminal.onOutput = (data) {
      if (!widget.isActive) return;
      // 멀티플렉서가 응답한 질의 원문도 미러에 남으므로 파싱 중 생긴 자동 응답만 막는다.
      if (_isParsingRemoteOutput && !widget.canRespondToDeviceQueries) return;
      if (_failureMessage == null) _stream?.write(data);
    };
    _terminal.onResize = (cols, rows, _, _) => _sendViewport(cols, rows);
    _terminal.onPrivateOSC = (code, args) {
      unawaited(_copyRemoteClipboard(code, args));
    };

    unawaited(_openStream());
  }

  @override
  void didUpdateWidget(PaneTerminalView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!oldWidget.isActive && widget.isActive) {
      _lastSentSize = null;
      _sendViewport(_terminal.viewWidth, _terminal.viewHeight);
    }
    if (widget.canResize &&
        (widget.pane.width != oldWidget.pane.width ||
            widget.pane.height != oldWidget.pane.height)) {
      _lastSentSize = null;
      _sendViewport(_terminal.viewWidth, _terminal.viewHeight);
    }

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

  /// 손가락이 닿은 칸. 휠 보고에 실어야 프로그램이 그 자리의 영역을 스크롤한다
  xterm.CellOffset? _readTouchedCell() {
    final position = _touchedPosition;
    final cellSize = _cellSize;
    if (position == null || cellSize == null) return null;
    return xterm.CellOffset(
      (position.dx ~/ cellSize.width).clamp(0, _terminal.viewWidth - 1),
      (position.dy ~/ cellSize.height).clamp(0, _terminal.viewHeight - 1),
    );
  }

  void _startTouch(PointerDownEvent event) {
    _touchedPosition = event.localPosition;
    _hasRequestedHistoryForTouch = false;
  }

  /// 맨 아래에서 지난 출력을 올려 보기 시작하면 데스크탑에 최신 이력을 한 번 요청한다.
  ///
  /// 화면 프레임은 보이는 칸만 덮어써서 위로 밀려난 줄이 폰에 쌓이지 않는다.
  /// 요청하지 않으면 올려 본 내용이 처음 연결했을 때의 출력에 머문다.
  void _requestHistoryWhenScrollingBack(PointerMoveEvent event) {
    final touchStart = _touchedPosition;
    if (!widget.canRefreshHistory ||
        _hasRequestedHistoryForTouch ||
        touchStart == null ||
        _terminal.isUsingAltBuffer ||
        event.localPosition.dy - touchStart.dy <= kTouchSlop) {
      return;
    }
    _hasRequestedHistoryForTouch = true;
    final isShowingLatest =
        !_verticalScroll.hasClients ||
        _verticalScroll.offset >= _verticalScroll.position.maxScrollExtent - 1;
    if (isShowingLatest) _stream?.refreshHistory();
  }

  void _sendViewport(int cols, int rows) {
    final size = (cols: cols, rows: rows);
    if (!widget.canResize ||
        !widget.isActive ||
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
      _cancelStreamPreparation();
      _firstFrameTimer?.cancel();
      unawaited(_subscription?.cancel());
      unawaited(_stream?.close());
      _stream = null;
    }
  }

  Future<void> _openStream() async {
    final generation = ++_streamGeneration;
    _cancelStreamPreparation();
    final cancellation = Completer<void>();
    _streamCancellation = cancellation;
    _pendingEscape = '';
    _deferredOutput.clear();
    _terminalController.clearSelection();
    _firstFrameTimer = Timer(_firstFrameTimeout, () {
      if (generation != _streamGeneration) return;
      _streamGeneration += 1;
      _cancelStreamPreparation();
      unawaited(_subscription?.cancel());
      unawaited(_stream?.close());
      _stream = null;
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
      final stream = await ref
          .read(desktopClientProvider)
          .openPaneStream(
            connection,
            widget.taskId,
            widget.pane.id,
            cancelled: cancellation.future,
            canPrepareDirectConnection: widget.canPrepareDirectConnection,
          );
      if (!mounted ||
          _isSuspended ||
          generation != _streamGeneration ||
          _failureMessage != null) {
        unawaited(stream.close());
        return;
      }
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
    if (_terminalController.selection != null &&
        (_deferredOutput.isNotEmpty || data.startsWith(_screenRedrawPrefix))) {
      // tmux 원시 스트림의 clear도 같은 접두사로 시작하므로, 보류를 시작한 뒤 오는 청크는 모두 순서대로 쌓는다.
      // 화면을 처음부터 다시 그리는 청크가 오면 그 앞의 보류분은 화면에 남지 않으므로 버려 메모리를 묶어 둔다.
      if (data.startsWith(_screenRedrawPrefix)) _deferredOutput.clear();
      _deferredOutput.add(data);
      return;
    }
    final lineCountBeforeWrite = _terminal.buffer.height;
    _isParsingRemoteOutput = true;
    try {
      _terminal.write(_normalizeLegacyTitles(data));
    } finally {
      _isParsingRemoteOutput = false;
    }
    if (widget.canRefreshHistory && data.startsWith(_screenRedrawPrefix)) {
      _keepDistanceFromLatest(lineCountBeforeWrite);
    }
  }

  /// 다시 받은 이력이 더 길면 늘어난 만큼 화면을 내려, 맨 아래에서 올려 본 거리를 지킨다.
  ///
  /// 그대로 두면 새로 받은 줄이 보던 자리와 최신 화면 사이에 끼어, 방금 지나간 줄 대신 처음 연결했을 때의 출력이 보인다.
  void _keepDistanceFromLatest(int lineCountBeforeWrite) {
    final cellSize = _cellSize;
    if (cellSize == null || !_verticalScroll.hasClients) return;
    final position = _verticalScroll.position;
    // 맨 아래를 보는 중이면 터미널이 스스로 최신 줄을 따라간다
    if (position.pixels >= position.maxScrollExtent - 1) return;
    final addedLineCount = _terminal.buffer.height - lineCountBeforeWrite;
    position.correctBy(addedLineCount * cellSize.height);
  }

  void _applyDeferredRedraw() {
    if (_terminalController.selection != null) return;
    final deferredOutput = List.of(_deferredOutput);
    _deferredOutput.clear();
    deferredOutput.forEach(_receiveOutput);
  }

  Future<void> _copyRemoteClipboard(String code, List<String> args) async {
    if (!mounted || _isSuspended || code != '52' || args.length != 2) return;
    final encoded = args[1];
    if (encoded.isEmpty ||
        encoded == '?' ||
        encoded.length > _maxClipboardPayloadLength) {
      return;
    }
    try {
      final text = utf8.decode(base64Decode(encoded), allowMalformed: true);
      await Clipboard.setData(ClipboardData(text: text));
    } on FormatException {
      // 잘못된 원격 요청 때문에 뒤에 오는 터미널 출력이 끊기지 않도록 무시한다.
    } on PlatformException catch (error) {
      debugPrint('터미널 클립보드 복사 실패: ${error.code}');
    }
  }

  Future<void> _copySelection() async {
    final selection = _terminalController.selection;
    if (selection == null) return;
    final text = _terminal.buffer.getText(selection);
    try {
      await Clipboard.setData(ClipboardData(text: text));
      if (mounted) _terminalController.clearSelection();
    } on PlatformException catch (error) {
      debugPrint('터미널 선택 복사 실패: ${error.code}');
    }
  }

  String _normalizeLegacyTitles(String data) {
    final combined = '$_pendingEscape$data';
    _pendingEscape = combined.endsWith('\x1b') ? '\x1b' : '';
    final ready = combined.substring(
      0,
      combined.length - _pendingEscape.length,
    );
    // xterm 4는 Screen의 ESC k 제목을 본문으로 그리므로 같은 제목 계약의 OSC 2로 바꾼다.
    // https://www.gnu.org/software/screen/manual/screen.html#Title-Prompts
    return ready.replaceAll('\x1bk', '\x1b]2;');
  }

  void _cancelStreamPreparation() {
    final cancellation = _streamCancellation;
    _streamCancellation = null;
    if (cancellation != null && !cancellation.isCompleted) {
      cancellation.complete();
    }
  }

  Future<void> _retryStream() async {
    _streamGeneration += 1;
    _cancelStreamPreparation();
    _firstFrameTimer?.cancel();
    unawaited(_subscription?.cancel());
    unawaited(_stream?.close());
    if (!mounted) return;
    _stream = null;
    setState(() {
      _failureMessage = null;
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

    /// 데스크탑이 세션 종류를 바꿨다. 이 pane은 이전 세션의 것이므로 태스크의 pane을 새로 받아 새 세션에 붙는다
    if (_stream?.readCloseCode() == taskSessionChangedCloseCode) {
      ref.invalidate(surfacesProvider(widget.taskId));
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
    _cancelStreamPreparation();
    _horizontalScroll.dispose();
    _verticalScroll.dispose();
    _terminalController.removeListener(_applyDeferredRedraw);
    _terminalController.dispose();
    unawaited(_subscription?.cancel());
    unawaited(_stream?.close());
    super.dispose();
  }

  Future<void> _runShortcut(String action) async {
    if (_isShortcutBusy || _isSuspended) return;
    setState(() => _isShortcutBusy = true);
    try {
      final screen = await ref
          .read(surfacesProvider(widget.taskId).notifier)
          .runShortcut(widget.pane.id, action);
      if (mounted && screen != null) _receiveOutput(screen);
    } catch (_) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(context.tr('단축키를 실행하지 못했습니다. 연결과 데스크탑 버전을 확인해 주세요.')),
          ),
        );
      }
    } finally {
      if (mounted) setState(() => _isShortcutBusy = false);
    }
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
    _cellSize = Size(metrics.width / 10, metrics.height);
    metrics.dispose();

    final terminalView = ColoredBox(
      color: AppColors.terminalBg,
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
                width: !widget.canResize && terminalWidth > constraints.maxWidth
                    ? terminalWidth
                    : constraints.maxWidth,
                height: constraints.maxHeight,
                child: MediaQuery.removePadding(
                  context: context,
                  removeLeft: true,
                  removeRight: true,
                  removeTop: true,
                  removeBottom: true,
                  child: Semantics(
                    identifier: 'pane-terminal',
                    label: '터미널 입력',
                    child: Listener(
                      onPointerDown: _startTouch,
                      onPointerMove: _requestHistoryWhenScrollingBack,
                      child: xterm.TerminalView(
                        _terminal,
                        controller: _terminalController,
                        autoResize: widget.canResize,
                        hardwareKeyboardOnly: true,
                        onKeyEvent: (_, event) {
                          if (widget.isActive &&
                              widget.onToggleChat != null &&
                              paneChatShortcut(Theme.of(context).platform)
                                  .accepts(event, HardwareKeyboard.instance)) {
                            widget.onToggleChat!();
                            return KeyEventResult.handled;
                          }
                          return KeyEventResult.ignored;
                        },
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
    );
    return TerminalSoftKeyboard(
      terminal: _terminal,
      controller: _terminalController,
      sessionType: widget.sessionType,
      onSessionAction: _runShortcut,
      isBusy:
          _isShortcutBusy ||
          ref.watch(
            surfacesProvider(widget.taskId)
                .select((state) => state.asData?.value.isMutating ?? false),
          ),
      child: Stack(
        children: [
          terminalView,
          Positioned(
            top: 8,
            right: 8,
            child: ListenableBuilder(
              listenable: _terminalController,
              builder: (context, _) {
                if (_terminalController.selection == null) {
                  return const SizedBox.shrink();
                }
                return Material(
                  color: AppColors.bgSurface,
                  borderRadius: BorderRadius.circular(8),
                  child: TextButton.icon(
                    onPressed: _copySelection,
                    style: TextButton.styleFrom(
                      minimumSize: const Size(48, 48),
                    ),
                    icon: const Icon(Icons.copy),
                    label: Text(context.tr('복사')),
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
                context.tr(message),
                textAlign: TextAlign.center,
                style: Theme.of(context).textTheme.bodyMedium
                    ?.copyWith(color: AppColors.textPrimary),
              ),
              const SizedBox(height: 16),
              TextButton(
                onPressed: onRetry,
                style: TextButton.styleFrom(
                  foregroundColor: AppColors.textPrimary,
                ),
                child: Text(context.tr('다시 연결')),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

// xterm 4.0.0의 휠 보고에는 결함이 둘 있다. 휠 ID에 Shift 비트가 잘못 포함되어 원격 프로그램이 스크롤을 무시하고,
// 화면 전체 기준 위치를 터미널 안 위치로 읽어 휠이 실제로 닿은 칸보다 아래쪽 칸을 가리킨다.
// https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h2-Wheel-mice
class _TerminalWheelMouseHandler extends xterm.TerminalMouseHandler {
  const _TerminalWheelMouseHandler(this._readTouchedCell);

  final xterm.CellOffset? Function() _readTouchedCell;

  @override
  String? call(xterm.TerminalMouseEvent event) {
    final touchedCell = event.button.isWheel ? _readTouchedCell() : null;
    final locatedEvent = touchedCell == null
        ? event
        : xterm.TerminalMouseEvent(
            button: event.button,
            buttonState: event.buttonState,
            position: touchedCell,
            state: event.state,
            platform: event.platform,
          );
    final output = xterm.defaultMouseHandler(locatedEvent);
    if (output == null || !event.button.isWheel) return output;
    final button = event.button.id - 4;
    return switch (event.state.mouseReportMode) {
      xterm.MouseReportMode.sgr => output.replaceFirst(
        '[<${event.button.id};',
        '[<$button;',
      ),
      xterm.MouseReportMode.urxvt => output.replaceFirst(
        '[${event.button.id + 32};',
        '[${button + 32};',
      ),
      xterm.MouseReportMode.normal || xterm.MouseReportMode.utf =>
        output.replaceRange(3, 4, String.fromCharCode(button + 32)),
    };
  }
}
