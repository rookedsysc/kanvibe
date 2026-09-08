import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../../../common/constants/app_theme.dart';
import '../../../common/providers.dart';
import '../domain/mirror_pane.dart';
import 'widgets/pane_terminal_view.dart';
import 'widgets/window_pane_composition.dart';

/// 태스크 하나의 터미널 화면.
///
/// 탭이 무엇을 가리키는지가 기기에 따라 다르다.
/// 폰에서는 탭 하나가 pane 하나라서, 고른 pane이 화면을 가득 채운다.
/// 태블릿에서는 탭 하나가 window 하나라서, 그 window의 pane이 데스크탑과 같은 배치로 함께 보인다.
class TaskDetailScreen extends ConsumerStatefulWidget {
  const TaskDetailScreen({
    required this.taskId,
    required this.taskTitle,
    super.key,
  });

  final String taskId;
  final String taskTitle;

  @override
  ConsumerState<TaskDetailScreen> createState() => _TaskDetailScreenState();
}

class _TaskDetailScreenState extends ConsumerState<TaskDetailScreen> {
  /// 고른 탭의 위치. 폰과 태블릿은 탭 목록의 길이가 달라서 화면이 바뀌면 다시 맞춘다
  int _selectedIndex = 0;

  @override
  Widget build(BuildContext context) {
    final surfaces = ref.watch(surfacesProvider(widget.taskId));

    return Scaffold(
      appBar: AppBar(
        title: Text(
          widget.taskTitle,
          style: Theme.of(context).textTheme.titleMedium,
        ),
      ),
      body: SafeArea(
        child: surfaces.when(
          loading: () => const Center(child: CircularProgressIndicator()),
          error: (error, _) => _SurfacesUnavailable(
            reason: error is SurfaceUnavailableException
                ? error.reason
                : SurfaceUnavailableReason.unknown,
            onRetry: () => ref.invalidate(surfacesProvider(widget.taskId)),
          ),
          data: (data) => LayoutBuilder(
            builder: (context, constraints) =>
                _buildSurfaces(data, isTabletLayout(constraints)),
          ),
        ),
      ),
    );
  }

  Widget _buildSurfaces(TaskSurfaces surfaces, bool isTablet) {
    final tabs = isTablet
        ? [for (final tab in surfaces.tabs) _SurfaceTab.window(tab)]
        : [for (final pane in surfaces.allPanes) _SurfaceTab.pane(pane)];

    if (tabs.isEmpty) {
      return _SurfacesUnavailable(
        reason: SurfaceUnavailableReason.sessionNotRunning,
        onRetry: () => ref.invalidate(surfacesProvider(widget.taskId)),
      );
    }

    /// 화면을 돌려 폰과 태블릿 배치를 오가면 탭 개수가 달라진다. 범위를 넘은 선택은 첫 탭으로 되돌린다
    final selectedIndex = _selectedIndex < tabs.length ? _selectedIndex : 0;

    return Column(
      children: [
        _SurfaceTabBar(
          tabs: tabs,
          selectedIndex: selectedIndex,
          onSelected: (index) => setState(() => _selectedIndex = index),
        ),
        Expanded(child: _buildSelectedSurface(tabs[selectedIndex])),
      ],
    );
  }

  Widget _buildSelectedSurface(_SurfaceTab tab) {
    final window = tab.window;
    if (window != null) {
      return WindowPaneComposition(taskId: widget.taskId, tab: window);
    }

    return PaneTerminalView(
      taskId: widget.taskId,

      /// 폰에서는 pane이 하나만 보이므로 어디로 입력할지 모호하지 않다
      pane: tab.pane!,
      key: ValueKey(tab.pane!.id),
    );
  }
}

/// 탭 바에 놓이는 항목 하나. 폰은 pane을, 태블릿은 window를 담는다
class _SurfaceTab {
  const _SurfaceTab._({
    required this.label,
    required this.detail,
    this.pane,
    this.window,
  });

  factory _SurfaceTab.pane(MirrorPane pane) =>
      _SurfaceTab._(label: pane.command, detail: pane.tabName, pane: pane);

  factory _SurfaceTab.window(MirrorTab window) => _SurfaceTab._(
    label: window.name,
    detail: '${window.panes.length} pane',
    window: window,
  );

  final String label;
  final String detail;
  final MirrorPane? pane;
  final MirrorTab? window;
}

class _SurfaceTabBar extends StatelessWidget {
  const _SurfaceTabBar({
    required this.tabs,
    required this.selectedIndex,
    required this.onSelected,
  });

  final List<_SurfaceTab> tabs;
  final int selectedIndex;
  final ValueChanged<int> onSelected;

  @override
  Widget build(BuildContext context) {
    return Container(
      height: AppSizes.minTouchTarget,
      decoration: const BoxDecoration(
        color: AppColors.terminalChrome,
        border: Border(bottom: BorderSide(color: AppColors.borderDefault)),
      ),
      child: ListView.builder(
        scrollDirection: Axis.horizontal,
        padding: const EdgeInsets.symmetric(horizontal: AppSizes.p8),
        itemCount: tabs.length,
        itemBuilder: (context, index) => _SurfaceTabButton(
          tab: tabs[index],
          isSelected: index == selectedIndex,
          onTap: () => onSelected(index),
        ),
      ),
    );
  }
}

class _SurfaceTabButton extends StatelessWidget {
  const _SurfaceTabButton({
    required this.tab,
    required this.isSelected,
    required this.onTap,
  });

  final _SurfaceTab tab;
  final bool isSelected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      selected: isSelected,
      button: true,
      label: '${tab.label} 탭',
      child: InkWell(
        onTap: onTap,
        child: Container(
          constraints: const BoxConstraints(minWidth: 96),
          alignment: Alignment.center,
          padding: const EdgeInsets.symmetric(horizontal: AppSizes.p12),
          decoration: BoxDecoration(
            border: Border(
              bottom: BorderSide(
                color: isSelected ? AppColors.brandPrimary : Colors.transparent,
                width: 2,
              ),
            ),
          ),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Flexible(
                child: Text(
                  tab.label,
                  overflow: TextOverflow.ellipsis,
                  style: monoStyle(
                    fontSize: 13,
                    color: isSelected
                        ? AppColors.textPrimary
                        : AppColors.textMuted,
                    fontWeight: isSelected ? FontWeight.w700 : FontWeight.w400,
                  ),
                ),
              ),
              const SizedBox(width: AppSizes.p8),
              Text(tab.detail, style: Theme.of(context).textTheme.labelSmall),
            ],
          ),
        ),
      ),
    );
  }
}

/// 세션을 비출 수 없을 때. 빈 터미널 대신 사유와 되돌아갈 길을 준다
class _SurfacesUnavailable extends StatelessWidget {
  const _SurfacesUnavailable({required this.reason, required this.onRetry});

  final SurfaceUnavailableReason reason;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(AppSizes.p24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              reason.message,
              textAlign: TextAlign.center,
              style: Theme.of(context).textTheme.bodyMedium,
            ),
            const SizedBox(height: AppSizes.p16),
            FilledButton(onPressed: onRetry, child: const Text('다시 시도')),
          ],
        ),
      ),
    );
  }
}
