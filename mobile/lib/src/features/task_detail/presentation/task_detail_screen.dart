import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../../../common/constants/app_theme.dart';
import '../../../common/providers.dart';
import '../../board/domain/board_task.dart';
import '../../board/domain/task_status.dart';
import '../../notifications/application/mobile_notification_controller.dart';
import '../../task_create/presentation/create_task_screen.dart';
import '../domain/mirror_pane.dart';
import 'surface_unavailable_message.dart';
import 'widgets/pane_terminal_view.dart';
import 'widgets/task_info_sheet.dart';
import 'widgets/task_status_sheet.dart';
import 'widgets/terminal_chrome_header.dart';

/// 태스크 하나의 터미널 화면.
///
/// 데스크탑 태스크 화면의 구성을 따른다. 그쪽은 왼쪽 레일(dock)에서 정보·상태·PR로 갈라지고
/// 본문은 `프로젝트 | 제목` 머리글 아래에 탭 바와 터미널을 놓는다. 여기서는 레일을 AppBar 아이콘으로,
/// 겹쳐 열리는 패널을 아래에서 올라오는 시트로 옮겼다. 폰에서 세로 레일이나 하단 바를 두면
/// 터미널이 쓸 세로 공간을 그만큼 잃는다.
///
/// 모든 화면 크기에서 pane 하나를 선택해 본문 전체에 표시한다.
class TaskDetailScreen extends ConsumerStatefulWidget {
  const TaskDetailScreen({required this.task, super.key});

  /// 보드에서 고른 태스크. 제목뿐 아니라 정보 시트가 그리는 칸까지 이 안에 들어 있다
  final BoardTask task;

  @override
  ConsumerState<TaskDetailScreen> createState() => _TaskDetailScreenState();
}

class _TaskDetailScreenState extends ConsumerState<TaskDetailScreen> {
  /// 고른 pane 탭의 위치
  int _selectedIndex = 0;

  /// 화면이 들고 있는 태스크. 상태를 옮기면 보드를 다시 받기 전에 여기부터 갈아 끼운다
  late BoardTask _task = widget.task;

  bool _isPreparingCreateTask = false;

  Future<void> _openCreateTask() async {
    if (_isPreparingCreateTask) {
      return;
    }
    setState(() => _isPreparingCreateTask = true);
    try {
      final board = await AsyncValue.guard(
        () => ref.refresh(boardProvider.future),
      );
      if (!mounted) {
        return;
      }
      if (board.hasError) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('프로젝트를 불러오지 못했습니다. 잠시 뒤 다시 시도해 주세요.')),
        );
        return;
      }
      final snapshot = board.requireValue;
      final project = snapshot.creatableProjects
          .where((project) => project.id == _task.projectId)
          .firstOrNull;
      if (project == null) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
            content: Text('이 태스크의 원본 프로젝트를 찾을 수 없습니다. 데스크탑에서 프로젝트를 확인해 주세요.'),
          ),
        );
        return;
      }
      final navigator = Navigator.of(context);
      final created = await navigator.push<BoardTask>(
        MaterialPageRoute(
          builder: (_) => CreateTaskScreen(
            projects: snapshot.creatableProjects,
            defaultSessionType: snapshot.defaultSessionType,
            defaultProjectId: project.id,
            defaultBaseBranch: _task.branchName?.isNotEmpty == true
                ? _task.branchName
                : _task.baseBranch,
          ),
        ),
      );
      if (!mounted || created == null) {
        return;
      }
      navigator.pushReplacement<void, void>(
        MaterialPageRoute(builder: (_) => TaskDetailScreen(task: created)),
      );
    } finally {
      if (mounted) {
        setState(() => _isPreparingCreateTask = false);
      }
    }
  }

  Future<void> _openInfoSheet() => showModalBottomSheet<void>(
    context: context,
    isScrollControlled: true,
    backgroundColor: AppColors.bgSurface,
    builder: (_) => TaskInfoSheet(task: _task),
  );

  Future<void> _openStatusSheet() async {
    final movedStatus = await showModalBottomSheet<TaskStatus>(
      context: context,
      backgroundColor: AppColors.bgSurface,
      builder: (_) => TaskStatusSheet(task: _task),
    );

    if (movedStatus != null && mounted) {
      setState(() => _task = _task.withStatus(movedStatus));
    }
  }

  /// PR은 앱 안에서 열지 않고 브라우저로 넘긴다. 로그인 세션이 이미 거기 있다.
  ///
  /// 열 앱이 없다는 것은 거짓으로 돌아오기도 하고 예외로 던져지기도 해서 둘 다 같은 안내로 받는다.
  /// 어느 쪽이든 사용자가 할 수 있는 일은 같고, 여기서 놓치면 버튼을 눌러도 아무 일이 없는 것처럼 보인다.
  Future<void> _openPullRequest() async {
    final messenger = ScaffoldMessenger.of(context);

    var opened = false;
    try {
      opened = await launchUrl(
        Uri.parse(_task.prUrl!),
        mode: LaunchMode.externalApplication,
      );
    } catch (_) {
      opened = false;
    }

    if (!opened) {
      messenger.showSnackBar(
        const SnackBar(content: Text('PR을 열 수 있는 앱이 없습니다.')),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(taskNotificationUpdatesProvider(_task.id), (_, next) {
      final status = TaskStatus.fromWire(next.value?.status ?? '');
      if (status != null && status != _task.status) {
        setState(() => _task = _task.withStatus(status));
      }
    });
    final surfaces = ref.watch(surfacesProvider(_task.id));

    return Scaffold(
      appBar: AppBar(
        title: Text(
          _task.title,
          style: Theme.of(context).textTheme.titleMedium,
        ),
        actions: [
          IconButton(
            onPressed: _openInfoSheet,
            icon: const Icon(Icons.info_outline),
            tooltip: '정보',
          ),
          IconButton(
            onPressed: _openStatusSheet,
            icon: const Icon(Icons.swap_horiz),
            tooltip: '상태 옮기기',
          ),
          if (_task.prUrl != null)
            IconButton(
              onPressed: _openPullRequest,
              icon: const Icon(Icons.call_merge),
              tooltip: 'PR 열기',
              color: AppColors.brandPrimary,
            ),
        ],
      ),
      body: SafeArea(
        child: Column(
          children: [
            if (MediaQuery.viewInsetsOf(context).bottom == 0)
              TerminalChromeHeader(
                task: _task,
                onCreateTask: _isPreparingCreateTask ? null : _openCreateTask,
              ),
            Expanded(
              child: surfaces.when(
                loading: () => const Center(child: CircularProgressIndicator()),
                error: (error, _) => _SurfacesUnavailable(
                  reason: error is SurfaceUnavailableException
                      ? error.reason
                      : SurfaceUnavailableReason.unknown,
                  onRetry: () => ref.invalidate(surfacesProvider(_task.id)),
                ),
                data: _buildSurfaces,
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildSurfaces(TaskSurfaces surfaces) {
    final tabs = surfaces.allPanes;

    if (tabs.isEmpty) {
      return _SurfacesUnavailable(
        reason: SurfaceUnavailableReason.sessionNotRunning,
        onRetry: () => ref.invalidate(surfacesProvider(_task.id)),
      );
    }

    /// 새 목록에서 선택이 범위를 벗어나면 첫 pane을 표시한다
    final selectedIndex = _selectedIndex < tabs.length ? _selectedIndex : 0;

    return Column(
      children: [
        _SurfaceTabBar(
          tabs: tabs,
          selectedIndex: selectedIndex,
          onSelected: (index) => setState(() => _selectedIndex = index),
        ),
        Expanded(child: _buildSelectedSurface(tabs[selectedIndex], surfaces)),
      ],
    );
  }

  Widget _buildSelectedSurface(MirrorPane pane, TaskSurfaces surfaces) =>
      PaneTerminalView(
        taskId: _task.id,
        pane: pane,
        canResize: surfaces.canResize,
        canRespondToDeviceQueries: surfaces.canRespondToDeviceQueries,
        key: ValueKey(pane.id),
      );
}

class _SurfaceTabBar extends StatelessWidget {
  const _SurfaceTabBar({
    required this.tabs,
    required this.selectedIndex,
    required this.onSelected,
  });

  final List<MirrorPane> tabs;
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

  final MirrorPane tab;
  final bool isSelected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      selected: isSelected,
      button: true,
      label: '${tab.command.isEmpty ? tab.id : tab.command} ${tab.id} 탭',
      child: InkWell(
        onTap: onTap,
        child: Container(
          width: 160,
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
          child: Column(
            mainAxisAlignment: MainAxisAlignment.center,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Flexible(
                child: Text(
                  tab.command.isEmpty ? tab.id : tab.command,
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
              const SizedBox(height: 2),
              Text(
                '${tab.tabName} ${tab.id}',
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.labelSmall,
              ),
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
              surfaceUnavailableMessage(reason),
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
