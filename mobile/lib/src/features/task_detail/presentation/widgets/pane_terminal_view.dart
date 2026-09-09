import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:xterm/xterm.dart' as xterm;

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_theme.dart';
import '../../../../common/network/desktop_client.dart';
import '../../../../common/providers.dart';
import '../../domain/mirror_pane.dart';
import '../surface_unavailable_message.dart';

/// pane 하나를 그대로 비추는 터미널.
///
/// 폰은 이것 하나를 화면 가득 띄우고, 태블릿은 window의 pane 개수만큼 늘어놓는다. 두 배치가 같은 위젯을 쓴다.
///
/// 터미널 크기는 데스크탑의 pane 크기를 그대로 따르고 화면에 맞춰 늘리거나 줄이지 않는다.
/// 모바일 화면 크기에 맞춰 다시 재면 줄바꿈이 데스크탑과 어긋나서, 같은 화면을 본다는 전제가 깨진다.
/// 대신 [FittedBox]로 통째로 축소해 "데스크탑 화면을 작게 보는" 결과가 되게 한다.
class PaneTerminalView extends ConsumerStatefulWidget {
  const PaneTerminalView({
    required this.taskId,
    required this.pane,
    this.isInteractive = true,
    super.key,
  });

  final String taskId;
  final MirrorPane pane;

  /// 태블릿에서 여러 pane을 늘어놓을 때는 어느 pane에 입력할지 모호해서 읽기 전용으로 둔다
  final bool isInteractive;

  @override
  ConsumerState<PaneTerminalView> createState() => _PaneTerminalViewState();
}

class _PaneTerminalViewState extends ConsumerState<PaneTerminalView> {
  late final xterm.Terminal _terminal;
  PaneStream? _stream;
  StreamSubscription<String>? _subscription;
  SurfaceUnavailableReason? _failureReason;

  @override
  void initState() {
    super.initState();
    _terminal = xterm.Terminal(maxLines: _scrollbackLines);
    _terminal.resize(widget.pane.width, widget.pane.height);

    _bindInput();

    unawaited(_openStream());
  }

  @override
  void didUpdateWidget(PaneTerminalView oldWidget) {
    super.didUpdateWidget(oldWidget);

    /// 태블릿에서 입력 대상이 옮겨 와도 이 위젯은 그대로 살아 있다.
    /// 키를 어디로 보낼지는 initState에서 한 번 정하고 끝낼 수 없다.
    if (widget.isInteractive != oldWidget.isInteractive) {
      _bindInput();
    }

    /// 데스크탑에서 pane을 분할하거나 창 크기를 바꾸면 같은 pane id로 새 좌표가 온다.
    /// key가 pane id라 element가 재사용되므로, 여기서 다시 재지 않으면 상자만 커지고 격자는 옛 크기로 남는다.
    if (widget.pane.width != oldWidget.pane.width ||
        widget.pane.height != oldWidget.pane.height) {
      _terminal.resize(widget.pane.width, widget.pane.height);
    }
  }

  /// 입력을 받기로 한 pane만 키를 데스크탑으로 흘려보낸다
  void _bindInput() {
    _terminal.onOutput = widget.isInteractive
        ? (data) => _stream?.write(data)
        : null;
  }

  /// 데스크탑 터미널이 보관하는 양과 비슷하게 잡는다. 더 늘리면 폰 메모리만 먹는다
  static const _scrollbackLines = 2000;

  Future<void> _openStream() async {
    final connection = await ref.read(connectionControllerProvider.future);
    if (connection == null || !mounted) {
      return;
    }

    final stream = ref
        .read(desktopClientProvider)
        .openPaneStream(connection, widget.taskId, widget.pane.id);

    _stream = stream;
    _subscription = stream.output.listen(
      _terminal.write,
      onError: (_) => _failStream(SurfaceUnavailableReason.unknown),
      onDone: _handleStreamClosed,
    );
  }

  /// 데스크탑이 이 기기를 끊었다면 다시 페어링하는 것 말고 사용자가 할 일이 없다.
  ///
  /// 그것을 세션 문제로 접어 보여 주면 사용자는 데스크탑에서 세션을 여닫으며 원인을 찾게 되고,
  /// 실제로 해야 할 일은 화면 어디에도 나타나지 않는다. 서버가 종료 코드로 나눠 보낸 사유가 여기서 쓰인다.
  void _handleStreamClosed() {
    if (_stream?.readCloseCode() == deviceUnpairedCloseCode) {
      unawaited(
        ref.read(connectionControllerProvider.notifier).forgetLocally(),
      );
      return;
    }

    _failStream(SurfaceUnavailableReason.sessionNotRunning);
  }

  void _failStream(SurfaceUnavailableReason reason) {
    if (mounted) {
      setState(() => _failureReason = reason);
    }
  }

  @override
  void dispose() {
    /// 마지막 구독자가 나가면 데스크탑이 pane에 걸어 둔 파이프도 함께 풀린다
    unawaited(_subscription?.cancel());
    unawaited(_stream?.close());
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final failureReason = _failureReason;
    if (failureReason != null) {
      return _PaneStreamFailure(failureReason);
    }

    return ColoredBox(
      color: AppColors.terminalBg,
      child: FittedBox(
        fit: BoxFit.contain,
        alignment: Alignment.topLeft,
        child: SizedBox(
          width: widget.pane.width * _cellWidth,
          height: widget.pane.height * _cellHeight,
          child: xterm.TerminalView(
            _terminal,

            /// 위젯 크기에 맞춰 다시 재면 데스크탑과 줄바꿈이 어긋난다
            autoResize: false,
            readOnly: !widget.isInteractive,
            backgroundOpacity: 0,
            theme: _terminalTheme,
            textStyle: const xterm.TerminalStyle(
              fontSize: _fontSize,
              fontFamily: monoFontFamily,
            ),
          ),
        ),
      ),
    );
  }

  /// [FittedBox]가 최종 크기를 정하므로 이 값은 비율만 맞으면 된다
  static const _fontSize = 14.0;
  static const _cellWidth = _fontSize * 0.6;
  static const _cellHeight = _fontSize * 1.35;
}

const _terminalTheme = xterm.TerminalTheme(
  cursor: AppColors.brandPrimary,
  selection: AppColors.brandSubtle,
  foreground: AppColors.terminalText,
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
  const _PaneStreamFailure(this.reason);

  final SurfaceUnavailableReason reason;

  @override
  Widget build(BuildContext context) {
    return ColoredBox(
      color: AppColors.terminalBg,
      child: Center(
        child: Text(
          surfaceUnavailableMessage(reason),
          textAlign: TextAlign.center,
          style: Theme.of(context).textTheme.bodySmall,
        ),
      ),
    );
  }
}
