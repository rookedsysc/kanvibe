import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_sizes.dart';
import '../../../../common/l10n/app_localizations.dart';
import '../../domain/mirror_pane.dart';
import 'pane_terminal_view.dart';

class MultiPaneTerminal extends StatelessWidget {
  const MultiPaneTerminal({
    required this.taskId,
    required this.surfaces,
    required this.selectedPane,
    required this.onSelectedPane,
    this.isActive = true,
    this.onToggleChat,
    super.key,
  });
  final String taskId;
  final TaskSurfaces surfaces;
  final MirrorPane selectedPane;
  final ValueChanged<String> onSelectedPane;
  final bool isActive;
  final VoidCallback? onToggleChat;

  Widget _terminal(MirrorPane pane) => PaneTerminalView(
    taskId: taskId,
    pane: pane,
    canResize: surfaces.canResize,
    sessionType: surfaces.sessionType,
    canRespondToDeviceQueries: surfaces.canRespondToDeviceQueries,
    isActive: isActive,
    onToggleChat: onToggleChat,
    key: ValueKey('${surfaces.sessionType.name}:${pane.id}'),
  );

  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (context, constraints) {
      final panes =
          surfaces.tabs
              .where((tab) => tab.id == selectedPane.tabId)
              .firstOrNull
              ?.panes ??
          [selectedPane];
      if (!isTabletLayout(constraints) || panes.length < 2) {
        return _terminal(selectedPane);
      }
      final right = panes.fold(
        1,
        (edge, pane) => math.max(edge, pane.left + math.max(pane.width, 1)),
      );
      final bottom = panes.fold(
        1,
        (edge, pane) => math.max(edge, pane.top + math.max(pane.height, 1)),
      );
      final minimumWidth = panes
          .map((pane) => 240 * right / math.max(pane.width, 1))
          .reduce(math.max);
      final minimumHeight = panes
          .map((pane) => 160 * bottom / math.max(pane.height, 1))
          .reduce(math.max);
      final width = math.max(constraints.maxWidth, minimumWidth);
      final height = math.max(constraints.maxHeight, minimumHeight);
      return SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: SingleChildScrollView(
          child: SizedBox(
            width: width,
            height: height,
            child: Stack(
              children: [
                for (final pane in panes)
                  Positioned(
                    left: math.max(0, pane.left) / right * width,
                    top: math.max(0, pane.top) / bottom * height,
                    width: math.max(1, pane.width) / right * width,
                    height: math.max(1, pane.height) / bottom * height,
                    child: Listener(
                      onPointerDown: (_) => onSelectedPane(pane.id),
                      child: Container(
                        key: ValueKey('multi-pane-${pane.id}'),
                        decoration: BoxDecoration(
                          border: Border.all(
                            color: pane.id == selectedPane.id
                                ? AppColors.brandPrimary
                                : context.colors.borderDefault,
                            width: 2,
                          ),
                        ),
                        child: Column(
                          children: [
                            SizedBox(
                              height: 28,
                              child: Align(
                                alignment: Alignment.centerLeft,
                                child: Text(
                                  '${pane.command.isEmpty ? context.tr('터미널') : pane.command} · ${pane.id}',
                                  maxLines: 1,
                                  overflow: TextOverflow.ellipsis,
                                ),
                              ),
                            ),
                            Expanded(child: ClipRect(child: _terminal(pane))),
                          ],
                        ),
                      ),
                    ),
                  ),
              ],
            ),
          ),
        ),
      );
    },
  );
}
