import 'package:flutter/material.dart';

import '../../../../common/constants/app_colors.dart';
import '../../domain/mirror_pane.dart';
import 'pane_terminal_view.dart';

/// 탭 하나(tmux window / zellij tab)를 데스크탑에 보이는 배치 그대로 조립한다.
///
/// 태블릿이 쓰는 화면이다. 멀티플렉서가 알려 준 좌표를 셀 단위로 받아 화면 크기에 맞춰 비율만 늘린다.
/// 배치를 다시 계산하지 않는 이유는, 데스크탑과 같은 화면이 보여야 한다는 것이 이 화면의 요구사항이기 때문이다.
///
/// 여러 pane이 함께 보이므로 입력이 어디로 갈지는 사용자가 정한다. 누른 pane 하나만 키보드를 받고
/// 나머지는 읽기 전용이라, 태블릿에서도 보기만 하는 것이 아니라 고른 pane에 바로 칠 수 있다.
class WindowPaneComposition extends StatefulWidget {
  const WindowPaneComposition({
    required this.taskId,
    required this.tab,
    super.key,
  });

  final String taskId;
  final MirrorTab tab;

  @override
  State<WindowPaneComposition> createState() => _WindowPaneCompositionState();
}

class _WindowPaneCompositionState extends State<WindowPaneComposition> {
  String? _focusedPaneId;

  /// 처음에는 멀티플렉서가 알려 준 첫 pane이 입력을 받는다
  String? get _activePaneId =>
      _focusedPaneId ??
      (widget.tab.panes.isEmpty ? null : widget.tab.panes.first.id);

  @override
  void didUpdateWidget(WindowPaneComposition oldWidget) {
    super.didUpdateWidget(oldWidget);

    /// 다른 window로 옮겨 가면 이전 window의 pane을 가리키던 선택은 버린다
    if (oldWidget.tab.id != widget.tab.id) {
      _focusedPaneId = null;
    }
  }

  @override
  Widget build(BuildContext context) {
    if (widget.tab.panes.isEmpty) {
      return const SizedBox.shrink();
    }

    final grid = _WindowGrid.measure(widget.tab.panes);

    return LayoutBuilder(
      builder: (context, constraints) {
        final cellWidth = constraints.maxWidth / grid.columns;
        final cellHeight = constraints.maxHeight / grid.rows;

        return Stack(
          children: [
            for (final pane in widget.tab.panes)
              Positioned(
                left: pane.left * cellWidth,
                top: pane.top * cellHeight,
                width: pane.width * cellWidth,
                height: pane.height * cellHeight,
                child: _PaneFrame(
                  isFocused: pane.id == _activePaneId,
                  onTap: () => setState(() => _focusedPaneId = pane.id),
                  child: PaneTerminalView(
                    taskId: widget.taskId,
                    pane: pane,
                    isInteractive: pane.id == _activePaneId,
                    key: ValueKey(pane.id),
                  ),
                ),
              ),
          ],
        );
      },
    );
  }
}

/// window 전체가 몇 칸인지. pane의 오른쪽·아래 끝 중 가장 먼 곳이 곧 전체 크기다
class _WindowGrid {
  const _WindowGrid({required this.columns, required this.rows});

  final int columns;
  final int rows;

  static _WindowGrid measure(List<MirrorPane> panes) {
    var columns = 1;
    var rows = 1;

    for (final pane in panes) {
      final right = pane.left + pane.width;
      final bottom = pane.top + pane.height;
      if (right > columns) {
        columns = right;
      }
      if (bottom > rows) {
        rows = bottom;
      }
    }

    return _WindowGrid(columns: columns, rows: rows);
  }
}

/// pane 사이 경계이자 입력 대상 표시. 멀티플렉서의 구분선을 대신한다
class _PaneFrame extends StatelessWidget {
  const _PaneFrame({
    required this.isFocused,
    required this.onTap,
    required this.child,
  });

  final bool isFocused;
  final VoidCallback onTap;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final framed = DecoratedBox(
      decoration: BoxDecoration(
        border: Border.all(
          color: isFocused ? AppColors.brandPrimary : AppColors.borderDefault,
        ),
        color: AppColors.terminalBg,
      ),
      child: child,
    );

    return Semantics(
      selected: isFocused,
      label: isFocused ? '입력 중인 pane' : '누르면 이 pane에 입력합니다',
      child: isFocused
          /// 입력 중인 pane의 탭은 터미널이 직접 받아야 커서 이동과 선택이 동작한다
          ? framed
          : GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTap: onTap,
              child: IgnorePointer(
                /// 터미널도 탭을 인식해서, 막지 않으면 제스처 경합에서 터미널이 이겨 입력 대상이 옮겨지지 않는다.
                /// 어차피 읽기 전용으로 비추는 중이라 포인터를 받을 이유가 없다.
                child: framed,
              ),
            ),
    );
  }
}
