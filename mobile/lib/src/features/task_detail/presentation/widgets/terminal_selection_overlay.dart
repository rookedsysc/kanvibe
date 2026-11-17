import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/cupertino.dart' show cupertinoTextSelectionControls;
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:xterm/xterm.dart' as xterm;

import '../../../../common/l10n/app_localizations.dart';

/// 터미널 글자 선택에 손잡이와 복사·붙여넣기 버튼 줄을 얹는다.
///
/// 선택 범위는 xterm의 [xterm.TerminalController]가 그대로 가진다. 여기서는 그 범위를 화면 좌표로 옮겨
/// 손잡이를 그리고, 손잡이를 끈 자리의 칸으로 범위의 끝을 옮긴다. 손잡이와 버튼 줄은 Flutter가
/// 글자 입력칸에 쓰는 것과 같은 위젯이라 플랫폼의 기본 모양을 따른다.
class TerminalSelectionOverlay extends StatefulWidget {
  const TerminalSelectionOverlay({
    required this.terminal,
    required this.controller,
    required this.terminalViewKey,
    required this.verticalScroll,
    required this.horizontalScroll,
    required this.isActive,
    required this.onCopy,
    required this.onPaste,
    required this.child,
    super.key,
  });

  final xterm.Terminal terminal;
  final xterm.TerminalController controller;
  final GlobalKey<xterm.TerminalViewState> terminalViewKey;
  final ScrollController verticalScroll;
  final ScrollController horizontalScroll;

  /// 화면에 보이는 터미널인지. 가려지면 최상위 레이어에 띄운 버튼 줄이 다른 화면 위에 남지 않게 내린다
  final bool isActive;
  final VoidCallback onCopy;
  final VoidCallback onPaste;
  final Widget child;

  @override
  State<TerminalSelectionOverlay> createState() =>
      _TerminalSelectionOverlayState();
}

/// 선택 범위의 양끝이 화면에서 놓이는 자리. 각 점은 그 줄의 아래쪽이며 전역 좌표다
typedef _SelectionPlacement = ({
  Offset start,
  Offset end,
  Rect terminalBounds,
  double lineHeight,
});

class _TerminalSelectionOverlayState extends State<TerminalSelectionOverlay> {
  static const _startHandleKey = ValueKey('terminal-selection-start-handle');
  static const _endHandleKey = ValueKey('terminal-selection-end-handle');

  /// 손잡이 그림은 한 변이 22px 안팎이라 손가락으로 잡기 어렵다. 사방으로 이만큼 넓혀 받는다
  static const _handleTouchPadding = 13.0;

  final _toolbar = ContextMenuController();

  /// 버튼 줄은 최상위 레이어에 따로 그려진다. 같은 묶음으로 표시해 버튼을 누르는 것이 바깥 터치로 읽히지 않게 한다
  final _toolbarTapGroup = Object();

  int _touchCount = 0;
  Offset? _pressPosition;
  Timer? _longPressTimer;
  bool _hasPressedLong = false;
  bool _wasToolbarWantedAtPress = false;
  bool _hasSelectionChangedDuringPress = false;

  bool _hadSelection = false;
  bool _isToolbarWanted = false;
  bool _isToolbarSyncScheduled = false;

  /// 글자가 없는 곳을 길게 눌러 붙여넣기만 띄울 자리
  Offset? _pasteAnchor;

  /// 손잡이를 끄는 동안 움직이지 않는 쪽 끝. 끄는 중에 출력이 밀려도 같은 글자를 가리키도록 anchor로 잡는다
  xterm.CellAnchor? _dragFixedEnd;
  Offset _dragGrabOffset = Offset.zero;

  @override
  void initState() {
    super.initState();
    widget.controller.addListener(_handleSelectionChanged);
    widget.verticalScroll.addListener(_handleViewChanged);
    widget.horizontalScroll.addListener(_handleViewChanged);
    widget.terminal.addListener(_handleViewChanged);
  }

  @override
  void didUpdateWidget(TerminalSelectionOverlay oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.isActive && !widget.isActive) _dismissToolbar();
  }

  @override
  void dispose() {
    widget.controller.removeListener(_handleSelectionChanged);
    widget.verticalScroll.removeListener(_handleViewChanged);
    widget.horizontalScroll.removeListener(_handleViewChanged);
    widget.terminal.removeListener(_handleViewChanged);
    _longPressTimer?.cancel();
    _dragFixedEnd?.dispose();
    _toolbar.remove();
    super.dispose();
  }

  void _handleSelectionChanged() {
    if (widget.controller.selection == null) {
      if (_pasteAnchor == null) _dismissToolbar();
    } else {
      _pasteAnchor = null;
      if (_touchCount == 0 && _dragFixedEnd == null) {
        _isToolbarWanted = true;
      } else {
        _hasSelectionChangedDuringPress = true;
      }
    }
    _handleViewChanged();
  }

  /// 선택 범위나 스크롤 위치, 터미널 내용이 바뀌면 손잡이와 버튼 줄을 선택 범위의 새 자리로 옮긴다
  void _handleViewChanged() {
    if (!mounted) return;
    final hasSelection = widget.controller.selection != null;
    // 선택이 없는 평소에는 출력이 올 때마다 다시 그리지 않는다.
    if (hasSelection || _hadSelection) setState(() {});
    _hadSelection = hasSelection;
    if (_isToolbarWanted) _scheduleToolbarSync();
  }

  void _startTerminalPress(PointerDownEvent event) {
    _touchCount += 1;
    _wasToolbarWantedAtPress = _isToolbarWanted;
    _hasSelectionChangedDuringPress = false;
    _dismissToolbar();
    _pressPosition = event.position;
    _hasPressedLong = false;
    _longPressTimer?.cancel();
    _longPressTimer = Timer(kLongPressTimeout, () => _hasPressedLong = true);
  }

  void _trackTerminalPress(PointerMoveEvent event) {
    final pressPosition = _pressPosition;
    if (pressPosition == null ||
        (event.position - pressPosition).distance <= kTouchSlop) {
      return;
    }
    _pressPosition = null;
    _longPressTimer?.cancel();
  }

  void _endTerminalPress(PointerEvent event) {
    _touchCount = math.max(0, _touchCount - 1);
    _longPressTimer?.cancel();
    final pressPosition = _pressPosition;
    _pressPosition = null;
    if (_touchCount > 0) return;
    // xterm은 같은 터치 끝에서 탭으로 판정하면 선택을 지운다. 그 판정이 끝난 뒤의 선택 상태를 본다.
    scheduleMicrotask(() {
      if (!mounted) return;
      if (widget.controller.selection != null) {
        if (!_hasSelectionChangedDuringPress && !_wasToolbarWantedAtPress) {
          return;
        }
      } else if (_hasPressedLong && pressPosition != null) {
        _pasteAnchor = pressPosition;
      } else {
        return;
      }
      _isToolbarWanted = true;
      _scheduleToolbarSync();
    });
  }

  void _startHandleDrag(PointerDownEvent event, {required bool isStartHandle}) {
    final selection = widget.controller.selection?.normalized;
    final placement = _locateSelection();
    if (selection == null || placement == null) return;
    _dismissToolbar();
    _dragFixedEnd?.dispose();
    _dragFixedEnd = widget.terminal.buffer.createAnchorFromOffset(
      isStartHandle ? selection.end : selection.begin,
    );
    _dragGrabOffset =
        event.position - (isStartHandle ? placement.start : placement.end);
  }

  void _dragHandle(PointerMoveEvent event) {
    final fixedEnd = _dragFixedEnd?.offset;
    final viewState = widget.terminalViewKey.currentState;
    if (fixedEnd == null || viewState == null) return;
    final render = viewState.renderTerminal;
    final cellSize = render.cellSize;
    // 손잡이가 붙은 점은 줄의 아래쪽이라, 그 줄의 가운데 높이에서 칸을 찾는다.
    final lineCenter =
        render.globalToLocal(event.position - _dragGrabOffset) -
        Offset(0, cellSize.height / 2);
    // 화면이 따라 스크롤되지 않으므로 보이는 줄 안에서만 끝을 옮긴다.
    final probe = Offset(
      lineCenter.dx,
      lineCenter.dy.clamp(0, render.size.height - 1),
    );
    final cell = render.getCellOffset(probe);
    final isPastCellCenter =
        probe.dx - render.getOffset(cell).dx > cellSize.width / 2;
    final movedEnd = xterm.CellOffset(
      isPastCellCenter ? cell.x + 1 : cell.x,
      cell.y,
    );
    // 빈 범위가 되면 두 손잡이가 겹쳐 다시 잡을 수 없다.
    if (movedEnd == fixedEnd) return;
    final (begin, end) = movedEnd.isBefore(fixedEnd)
        ? (movedEnd, fixedEnd)
        : (fixedEnd, movedEnd);
    final buffer = widget.terminal.buffer;
    widget.controller.setSelection(
      buffer.createAnchorFromOffset(begin),
      buffer.createAnchorFromOffset(end),
    );
  }

  void _endHandleDrag() {
    if (_dragFixedEnd == null) return;
    _dragFixedEnd?.dispose();
    _dragFixedEnd = null;
    if (widget.controller.selection == null) return;
    _isToolbarWanted = true;
    _scheduleToolbarSync();
  }

  _SelectionPlacement? _locateSelection() {
    final selection = widget.controller.selection?.normalized;
    final viewState = widget.terminalViewKey.currentState;
    if (selection == null || viewState == null) return null;
    final render = viewState.renderTerminal;
    if (!render.hasSize) return null;
    final lineHeight = render.cellSize.height;
    Offset lineBottom(xterm.CellOffset cell) =>
        render.localToGlobal(render.getOffset(cell) + Offset(0, lineHeight));
    return (
      start: lineBottom(selection.begin),
      end: lineBottom(selection.end),
      terminalBounds: render.localToGlobal(Offset.zero) & render.size,
      lineHeight: lineHeight,
    );
  }

  /// 버튼 줄은 손잡이와 같은 프레임의 배치가 끝난 뒤에 옮겨야 선택 범위의 새 좌표를 읽는다
  void _scheduleToolbarSync() {
    if (_isToolbarSyncScheduled) return;
    _isToolbarSyncScheduled = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _isToolbarSyncScheduled = false;
      if (mounted) _syncToolbar();
    });
    WidgetsBinding.instance.ensureVisualUpdate();
  }

  void _syncToolbar() {
    final anchors = _isToolbarWanted && widget.isActive
        ? _locateToolbar()
        : null;
    if (anchors == null) {
      _toolbar.remove();
      return;
    }
    final buttonItems = [
      if (widget.controller.selection != null)
        ContextMenuButtonItem(
          type: ContextMenuButtonType.copy,
          label: context.tr('복사'),
          onPressed: () => _pressToolbarButton(widget.onCopy),
        ),
      ContextMenuButtonItem(
        type: ContextMenuButtonType.paste,
        label: context.tr('붙여넣기'),
        onPressed: () => _pressToolbarButton(widget.onPaste),
      ),
    ];
    _toolbar.show(
      context: context,
      contextMenuBuilder: (_) => TapRegion(
        groupId: _toolbarTapGroup,
        child: AdaptiveTextSelectionToolbar.buttonItems(
          anchors: anchors,
          buttonItems: buttonItems,
        ),
      ),
    );
  }

  /// 버튼 줄을 선택 위에, 자리가 없으면 아래에 붙일 기준점. 선택이 화면 밖으로 밀려나면 붙일 자리가 없다
  TextSelectionToolbarAnchors? _locateToolbar() {
    final placement = _locateSelection();
    if (placement == null) {
      final pasteAnchor = _pasteAnchor;
      return pasteAnchor == null
          ? null
          : TextSelectionToolbarAnchors(primaryAnchor: pasteAnchor);
    }
    final bounds = placement.terminalBounds;
    final selectionTop = placement.start.dy - placement.lineHeight;
    if (placement.end.dy < bounds.top || selectionTop > bounds.bottom) {
      return null;
    }
    final isSingleLine = placement.start.dy == placement.end.dy;
    final centerX = isSingleLine
        ? (placement.start.dx + placement.end.dx) / 2
        : bounds.center.dx;
    return TextSelectionToolbarAnchors(
      primaryAnchor: Offset(centerX, math.max(selectionTop, bounds.top)),
      secondaryAnchor: Offset(
        centerX,
        math.min(placement.end.dy, bounds.bottom),
      ),
    );
  }

  void _pressToolbarButton(VoidCallback action) {
    _dismissToolbar();
    action();
  }

  void _dismissToolbar() {
    _isToolbarWanted = false;
    _pasteAnchor = null;
    _toolbar.remove();
  }

  @override
  Widget build(BuildContext context) {
    return TapRegion(
      groupId: _toolbarTapGroup,
      onTapOutside: (_) => _dismissToolbar(),
      child: Stack(
        fit: StackFit.expand,
        clipBehavior: Clip.none,
        children: [
          Listener(
            behavior: HitTestBehavior.translucent,
            onPointerDown: _startTerminalPress,
            onPointerMove: _trackTerminalPress,
            onPointerUp: _endTerminalPress,
            onPointerCancel: _endTerminalPress,
            child: widget.child,
          ),
          ..._buildHandles(context),
        ],
      ),
    );
  }

  List<Widget> _buildHandles(BuildContext context) {
    final placement = _locateSelection();
    final overlayBox = context.findRenderObject();
    if (placement == null || overlayBox is! RenderBox) return const [];
    final controls = switch (Theme.of(context).platform) {
      TargetPlatform.iOS ||
      TargetPlatform.macOS => cupertinoTextSelectionControls,
      _ => materialTextSelectionControls,
    };
    final visibleBounds = placement.terminalBounds.intersect(
      Offset.zero & MediaQuery.sizeOf(context),
    );
    return [
      for (final (isStartHandle, linePoint) in [
        (true, placement.start),
        (false, placement.end),
      ])
        // 화면 밖으로 밀려난 끝의 손잡이는 터미널 바깥에 그려지므로 숨긴다. 끄는 중에 사라지면 터치의 끝을 받지 못하므로 그동안은 둔다.
        if (_dragFixedEnd != null ||
            placement.terminalBounds.inflate(1).contains(linePoint))
          _buildHandle(
            context,
            controls: controls,
            isStartHandle: isStartHandle,
            type: _chooseHandleType(
              controls,
              isStartHandle: isStartHandle,
              linePoint: linePoint,
              lineHeight: placement.lineHeight,
              visibleBounds: visibleBounds,
            ),
            linePoint: overlayBox.globalToLocal(linePoint),
            lineHeight: placement.lineHeight,
          ),
    ];
  }

  /// 시작 손잡이는 왼쪽으로, 끝 손잡이는 오른쪽으로 뻗는다. 화면 가장자리에 붙은 끝에서는 그 모양이 화면 밖에 그려져 보이지도 잡히지도 않으므로 안쪽으로 뻗는 모양으로 바꾼다
  TextSelectionHandleType _chooseHandleType(
    TextSelectionControls controls, {
    required bool isStartHandle,
    required Offset linePoint,
    required double lineHeight,
    required Rect visibleBounds,
  }) {
    final handleWidth = controls.getHandleSize(lineHeight).width;
    bool isInsideScreen(TextSelectionHandleType type) {
      final left = linePoint.dx - controls.getHandleAnchor(type, lineHeight).dx;
      return left >= visibleBounds.left &&
          left + handleWidth <= visibleBounds.right;
    }

    final (outward, inward) = isStartHandle
        ? (TextSelectionHandleType.left, TextSelectionHandleType.right)
        : (TextSelectionHandleType.right, TextSelectionHandleType.left);
    return !isInsideScreen(outward) && isInsideScreen(inward)
        ? inward
        : outward;
  }

  Widget _buildHandle(
    BuildContext context, {
    required TextSelectionControls controls,
    required bool isStartHandle,
    required TextSelectionHandleType type,
    required Offset linePoint,
    required double lineHeight,
  }) {
    final anchor = controls.getHandleAnchor(type, lineHeight);
    return Positioned(
      key: isStartHandle ? _startHandleKey : _endHandleKey,
      left: linePoint.dx - anchor.dx - _handleTouchPadding,
      top: linePoint.dy - anchor.dy - _handleTouchPadding,
      child: Semantics(
        identifier: isStartHandle
            ? 'terminal-selection-start-handle'
            : 'terminal-selection-end-handle',
        label: context.tr(isStartHandle ? '선택 시작 손잡이' : '선택 끝 손잡이'),
        child: RawGestureDetector(
          behavior: HitTestBehavior.opaque,
          gestures: {
            // 가로로 넘치는 pane에서는 바깥 스크롤이 옆으로 끄는 동작을 먼저 가져가므로, 손잡이에 닿는 순간 터치를 차지한다.
            EagerGestureRecognizer:
                GestureRecognizerFactoryWithHandlers<EagerGestureRecognizer>(
                  EagerGestureRecognizer.new,
                  (_) {},
                ),
          },
          child: Listener(
            behavior: HitTestBehavior.opaque,
            onPointerDown: (event) =>
                _startHandleDrag(event, isStartHandle: isStartHandle),
            onPointerMove: _dragHandle,
            onPointerUp: (_) => _endHandleDrag(),
            onPointerCancel: (_) => _endHandleDrag(),
            child: Padding(
              padding: const EdgeInsets.all(_handleTouchPadding),
              child: controls.buildHandle(context, type, lineHeight),
            ),
          ),
        ),
      ),
    );
  }
}
