import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../../../common/constants/app_theme.dart';
import '../../../common/network/desktop_command.dart';
import '../../../common/providers.dart';
import '../../board/domain/board_task.dart';
import '../../board/domain/task_status.dart';
import '../../chat_history/presentation/chat_history_screen.dart';
import '../../notifications/application/mobile_notification_controller.dart';
import '../../notifications/application/notification_inbox_controller.dart';
import '../../pane_chat/presentation/pane_chat_view.dart';
import '../../task_create/domain/session_type.dart';
import '../../task_create/presentation/create_task_screen.dart';
import '../domain/mirror_pane.dart';
import 'pane_chat_shortcut.dart';
import 'surface_unavailable_message.dart';
import 'widgets/pane_terminal_view.dart';
import 'widgets/task_info_sheet.dart';
import 'widgets/task_session_type_sheet.dart';
import 'widgets/task_status_sheet.dart';
import 'widgets/terminal_chrome_header.dart';

/// 프로젝트·브랜치와 태스크 동작을 상단에 모으고 선택한 pane을 표시한다.
class TaskDetailScreen extends ConsumerStatefulWidget {
  const TaskDetailScreen({required this.task, super.key});

  /// 보드에서 고른 태스크. 제목뿐 아니라 정보 시트가 그리는 칸까지 이 안에 들어 있다
  final BoardTask task;

  @override
  ConsumerState<TaskDetailScreen> createState() => _TaskDetailScreenState();
}

class _TaskDetailScreenState extends ConsumerState<TaskDetailScreen>
    with WidgetsBindingObserver {
  String? _selectedPaneId;
  final _chatModePaneIds = <String>{};
  final _chatPaneIds = <String>{};
  final _chatPaneNumbers = <String, int>{};

  /// 화면이 들고 있는 태스크. 상태를 옮기면 보드를 다시 받기 전에 여기부터 갈아 끼운다
  late BoardTask _task = widget.task;

  bool _isPreparingCreateTask = false;

  bool _isAppResumed =
      (WidgetsBinding.instance.lifecycleState ?? AppLifecycleState.resumed) ==
      AppLifecycleState.resumed;
  bool _wasViewingTask = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _markTaskNotificationsReadWhenViewing();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    _isAppResumed = state == AppLifecycleState.resumed;
    _markTaskNotificationsReadWhenViewing();
  }

  /// 데스크탑 상세 화면처럼 사용자가 이 태스크를 실제로 보게 된 순간에만 알림을 읽음 처리한다.
  /// 다른 화면이 위에 덮였거나 앱이 백그라운드인 동안 도착한 알림은 다시 볼 때까지 안읽음으로 남긴다.
  void _markTaskNotificationsReadWhenViewing() {
    final isViewingTask =
        _isAppResumed && (ModalRoute.isCurrentOf(context) ?? true);
    if (isViewingTask && !_wasViewingTask) {
      // 실패해도 다음에 화면을 다시 볼 때 같은 요청을 보내므로 화면 동작은 막지 않는다.
      unawaited(
        ref
            .read(notificationInboxProvider.notifier)
            .markTaskRead(_task.id)
            .catchError((Object _) {}),
      );
    }
    _wasViewingTask = isViewingTask;
  }

  void _setPaneChatMode(String paneId, bool enabled) {
    setState(() {
      if (enabled) {
        _chatModePaneIds.add(paneId);
        _chatPaneIds.add(paneId);
      } else {
        _chatModePaneIds.remove(paneId);
      }
    });
  }

  void _toggleSelectedPaneChat() {
    if (!(ModalRoute.isCurrentOf(context) ?? true)) return;
    final panes = ref.read(surfacesProvider(_task.id)).value?.surfaces.allPanes;
    if (panes == null || panes.isEmpty) return;
    final pane =
        panes.where((pane) => pane.id == _selectedPaneId).firstOrNull ??
        panes.first;
    _setPaneChatMode(pane.id, !_chatModePaneIds.contains(pane.id));
  }

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

  Future<void> _createTerminalTab() async {
    try {
      final paneId = await ref
          .read(surfacesProvider(_task.id).notifier)
          .createTab();
      if (mounted && paneId != null) {
        setState(() => _selectedPaneId = paneId);
      }
    } catch (_) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('터미널 탭을 추가하지 못했습니다. 잠시 뒤 다시 시도해 주세요.')),
      );
    }
  }

  /// 닫아도 되는지 정한다. 실행 중인 프로세스가 있거나 조회가 실패하면 사용자에게 한 번 더 묻는다
  Future<bool> _confirmCloseTab(MirrorPane tab) async {
    final controller = ref.read(surfacesProvider(_task.id).notifier);
    final List<String> processNames;
    try {
      processNames = await controller.fetchCloseProcesses(tab.id);
    } on DesktopUpdateRequiredException {
      _showNotice(desktopUpdateRequiredMessage);
      return false;
    } on DesktopDisconnectedException {
      _showNotice(desktopDisconnectedMessage);
      return false;
    } catch (_) {
      return _askCloseConfirmation(
        '실행 중인 프로세스를 확인하지 못했습니다. 탭을 닫으면 그 안의 프로세스가 종료됩니다.',
      );
    }
    if (processNames.isEmpty) {
      return true;
    }
    return _askCloseConfirmation(
      '${processNames.join(', ')} 실행 중인 탭을 닫으면 종료됩니다',
    );
  }

  Future<bool> _askCloseConfirmation(String message) async {
    if (!mounted) return false;
    final isConfirmed = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        backgroundColor: AppColors.bgSurface,
        title: const Text('탭을 닫을까요?'),
        content: Text(message),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(dialogContext).pop(false),
            child: const Text('취소'),
          ),
          TextButton(
            onPressed: () => Navigator.of(dialogContext).pop(true),
            style: TextButton.styleFrom(foregroundColor: AppColors.statusError),
            child: const Text('닫기'),
          ),
        ],
      ),
    );
    return isConfirmed ?? false;
  }

  /// 누른 탭 하나만 닫는다. 선택한 탭을 닫으면 앞 탭으로, 맨 앞이면 다음 탭으로 옮긴다
  Future<void> _closeTab(List<MirrorPane> tabs, MirrorPane tab) async {
    if (!await _confirmCloseTab(tab) || !mounted) {
      return;
    }

    final closedIndex = tabs.indexWhere((pane) => pane.id == tab.id);
    try {
      final surfaces = await ref
          .read(surfacesProvider(_task.id).notifier)
          .closeTab(tab.id);
      if (!mounted || surfaces == null) return;
      final isStillSelected = (_selectedPaneId ?? tabs.first.id) == tab.id;
      if (isStillSelected) {
        final neighbor = closedIndex > 0
            ? tabs[closedIndex - 1]
            : tabs[closedIndex + 1];
        setState(() => _selectedPaneId = neighbor.id);
      }
    } on DesktopCommandFailure catch (failure) {
      _showNotice(failure.message);
    } on DesktopUpdateRequiredException {
      _showNotice(desktopUpdateRequiredMessage);
    } on DesktopDisconnectedException {
      _showNotice(desktopDisconnectedMessage);
    } catch (_) {
      _showNotice('탭을 닫지 못했습니다. 잠시 뒤 다시 시도해 주세요.');
    }
  }

  void _showNotice(String message) {
    if (!mounted) return;
    ScaffoldMessenger.of(context)
        .showSnackBar(SnackBar(content: Text(message)));
  }

  Future<void> _openInfoSheet() => showModalBottomSheet<void>(
    context: context,
    isScrollControlled: true,
    backgroundColor: AppColors.bgSurface,
    builder: (_) => TaskInfoSheet(task: _task),
  );

  Future<void> _openStatusSheet() async {
    final result = await showModalBottomSheet<TaskStatusSheetResult>(
      context: context,
      backgroundColor: AppColors.bgSurface,
      builder: (_) => TaskStatusSheet(task: _task),
    );

    if (!mounted) {
      return;
    }
    switch (result) {
      case TaskStatusMoved(:final status):
        setState(() => _task = _task.withStatus(status));
      case TaskDeleted(:final failedPeerCount):
        // 앱 전체의 messenger에 띄워야 보드로 돌아간 뒤에도 경고가 보인다
        if (failedPeerCount > 0) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(
              content: Text(
                '다른 데스크탑 $failedPeerCount곳에는 아직 남아 있습니다. 그 데스크탑이 켜지면 직접 삭제해 주세요.',
              ),
            ),
          );
        }
        // 지운 태스크의 화면에는 비출 pane이 없으니 보드로 돌아간다
        Navigator.of(context).pop();
      case null:
        break;
    }
  }

  Future<void> _openSessionTypeSheet(SessionType currentSessionType) =>
      showModalBottomSheet<SessionType>(
        context: context,
        backgroundColor: AppColors.bgSurface,
        builder: (_) => TaskSessionTypeSheet(
          taskId: _task.id,
          currentSessionType: currentSessionType,
        ),
      );

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
    final headerActions =
        <({String id, String label, IconData icon, VoidCallback? onPressed})>[
          (
            id: 'create-task-from-current',
            label: _isPreparingCreateTask ? '불러오는 중…' : '새 태스크',
            icon: Icons.add_task,
            onPressed: _isPreparingCreateTask ? null : _openCreateTask,
          ),
          (
            id: 'chat-history',
            label: '대화 기록',
            icon: Icons.chat_bubble_outline,
            onPressed: () => Navigator.of(context).push(
              MaterialPageRoute<void>(
                builder: (_) => ChatHistoryScreen(taskId: _task.id),
              ),
            ),
          ),
          (
            id: 'task-info',
            label: '정보',
            icon: Icons.info_outline,
            onPressed: _openInfoSheet,
          ),
          (
            id: 'task-status',
            label: '상태 옮기기',
            icon: Icons.swap_horiz,
            onPressed: _openStatusSheet,
          ),
          (
            id: 'change-session-type',
            label: '세션 종류 바꾸기',
            icon: Icons.terminal,
            onPressed: surfaces.hasValue
                ? () => _openSessionTypeSheet(
                    surfaces.requireValue.surfaces.sessionType,
                  )
                : null,
          ),
          if (_task.prUrl != null)
            (
              id: 'task-pull-request',
              label: 'PR 열기',
              icon: Icons.call_merge,
              onPressed: _openPullRequest,
            ),
        ];

    final content = LayoutBuilder(
      builder: (context, constraints) {
        final isCompact = !isTabletLayout(constraints);
        return Scaffold(
          appBar: AppBar(
            titleSpacing: AppSizes.p8,
            leadingWidth: AppSizes.minTouchTarget,
            title: TerminalChromeHeader(task: _task),
            actions: [
              if (isCompact)
                Semantics(
                  identifier: 'task-more',
                  child: PopupMenuButton<VoidCallback>(
                    tooltip: '태스크 메뉴',
                    onSelected: (action) => action(),
                    itemBuilder: (_) => [
                      for (final action in headerActions)
                        PopupMenuItem<VoidCallback>(
                          value: action.onPressed,
                          enabled: action.onPressed != null,
                          child: Semantics(
                            identifier: action.id,
                            child: Tooltip(
                              message: action.label,
                              child: Row(
                                children: [
                                  Icon(
                                    action.icon,
                                    color: action.id == 'task-pull-request'
                                        ? AppColors.brandPrimary
                                        : null,
                                  ),
                                  const SizedBox(width: AppSizes.p12),
                                  Flexible(child: Text(action.label)),
                                ],
                              ),
                            ),
                          ),
                        ),
                    ],
                  ),
                )
              else
                for (final action in headerActions)
                  Semantics(
                    identifier: action.id,
                    child: IconButton(
                      onPressed: action.onPressed,
                      icon: Icon(action.icon),
                      tooltip: action.label,
                      color: action.id == 'task-pull-request'
                          ? AppColors.brandPrimary
                          : null,
                    ),
                  ),
            ],
          ),
          body: SafeArea(
            child: Column(
              children: [
                Expanded(
                  child: surfaces.when(
                    loading: () =>
                        const Center(child: CircularProgressIndicator()),
                    error: (error, _) => _SurfacesUnavailable(
                      reason: error is SurfaceUnavailableException
                          ? error.reason
                          : SurfaceUnavailableReason.unknown,
                      message: switch (error) {
                        DesktopUpdateRequiredException() =>
                          desktopUpdateRequiredMessage,
                        DesktopDisconnectedException() =>
                          desktopDisconnectedMessage,
                        DesktopCommandFailure() => error.message,
                        _ => null,
                      },
                      onRetry: () => ref
                          .read(surfacesProvider(_task.id).notifier)
                          .startSession(),
                    ),
                    data: (value) => _buildSurfaces(
                      value.surfaces,
                      isCreating: value.isMutating,
                    ),
                  ),
                ),
              ],
            ),
          ),
        );
      },
    );
    return CallbackShortcuts(
      bindings: {
        paneChatShortcut(Theme.of(context).platform): _toggleSelectedPaneChat,
      },
      child: Focus(autofocus: true, child: content),
    );
  }

  Widget _buildSurfaces(TaskSurfaces surfaces, {required bool isCreating}) {
    final tabs = surfaces.allPanes;
    final chatLabels = <String, String>{};
    for (final pane in tabs) {
      final number = _chatPaneNumbers.putIfAbsent(
        pane.id,
        () => _chatPaneNumbers.length + 1,
      );
      final name = [
        pane.tabName.trim(),
        pane.command.trim(),
        'pane',
      ].firstWhere((value) => value.isNotEmpty);
      chatLabels[pane.id] = '$number · $name';
    }

    if (tabs.isEmpty) {
      return _SurfacesUnavailable(
        reason: SurfaceUnavailableReason.sessionNotRunning,
        onRetry: () =>
            ref.read(surfacesProvider(_task.id).notifier).startSession(),
      );
    }

    final matchingIndex = tabs.indexWhere((pane) => pane.id == _selectedPaneId);
    final selectedIndex = matchingIndex < 0 ? 0 : matchingIndex;
    final selectedPaneId = tabs[selectedIndex].id;
    final chatMode = _chatModePaneIds.contains(selectedPaneId);

    return Column(
      children: [
        _SurfaceTabBar(
          tabs: tabs,
          chatLabels: _chatPaneIds.isNotEmpty ? chatLabels : const {},
          selectedIndex: selectedIndex,
          onSelected: (index) => setState(() {
            _selectedPaneId = tabs[index].id;
          }),
          onCreate: isCreating ? null : _createTerminalTab,
          // 탭이 하나뿐이면 세션이 끝나 버리므로 닫을 수 없다
          onClose: isCreating || tabs.length == 1
              ? null
              : (tab) => _closeTab(tabs, tab),
        ),
        Row(
          children: [
            Expanded(
              child: Semantics(
                identifier: 'pane-terminal-mode',
                selected: !chatMode,
                child: TextButton.icon(
                  onPressed: () => _setPaneChatMode(selectedPaneId, false),
                  icon: const Icon(Icons.terminal, size: AppSizes.p16),
                  label: const Text('터미널'),
                  style: TextButton.styleFrom(
                    foregroundColor: !chatMode
                        ? AppColors.brandPrimary
                        : AppColors.textSecondary,
                  ),
                ),
              ),
            ),
            Expanded(
              child: Semantics(
                identifier: 'pane-chat-mode',
                selected: chatMode,
                child: TextButton.icon(
                  onPressed: () => _setPaneChatMode(selectedPaneId, true),
                  icon: const Icon(
                    Icons.chat_bubble_outline,
                    size: AppSizes.p16,
                  ),
                  label: const Text('채팅'),
                  style: TextButton.styleFrom(
                    foregroundColor: chatMode
                        ? AppColors.brandPrimary
                        : AppColors.textSecondary,
                  ),
                ),
              ),
            ),
          ],
        ),
        Expanded(
          child: _buildSelectedSurface(
            tabs[selectedIndex],
            surfaces,
            chatLabels,
          ),
        ),
      ],
    );
  }

  /// 세션 종류가 바뀌면 pane id가 같더라도 다른 세션이므로 pane 화면을 새로 만들어 스트림을 다시 연다
  Widget _buildSelectedSurface(
    MirrorPane pane,
    TaskSurfaces surfaces,
    Map<String, String> chatLabels,
  ) {
    final routeActive = ModalRoute.isCurrentOf(context) ?? true;
    final chatMode = _chatModePaneIds.contains(pane.id);
    return KeyedSubtree(
      key: ValueKey(surfaces.sessionType),
      child: Stack(
        fit: StackFit.expand,
        children: [
          ExcludeFocus(
            excluding: chatMode || !routeActive,
            child: Offstage(
              offstage: chatMode,
              child: PaneTerminalView(
                taskId: _task.id,
                pane: pane,
                canResize: surfaces.canResize,
                sessionType: surfaces.sessionType,
                canRespondToDeviceQueries: surfaces.canRespondToDeviceQueries,
                isActive: !chatMode && routeActive,
                onToggleChat: _toggleSelectedPaneChat,
                key: ValueKey(pane.id),
              ),
            ),
          ),
          for (final chatPane in surfaces.allPanes.where(
            (entry) => _chatPaneIds.contains(entry.id),
          ))
            Offstage(
              key: ValueKey('pane-chat:${chatPane.id}'),
              offstage: !chatMode || chatPane.id != pane.id,
              child: ExcludeFocus(
                excluding: !chatMode || chatPane.id != pane.id || !routeActive,
                child: TickerMode(
                  enabled: chatMode && chatPane.id == pane.id && routeActive,
                  child: PaneChatView(
                    target: (taskId: _task.id, paneId: chatPane.id),
                    paneLabel: chatLabels[chatPane.id],
                    isActive: chatMode && chatPane.id == pane.id && routeActive,
                    onReturnToTerminal: () =>
                        _setPaneChatMode(chatPane.id, false),
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

class _SurfaceTabBar extends StatelessWidget {
  const _SurfaceTabBar({
    required this.tabs,
    required this.chatLabels,
    required this.selectedIndex,
    required this.onSelected,
    required this.onCreate,
    required this.onClose,
  });

  final List<MirrorPane> tabs;
  final Map<String, String> chatLabels;
  final int selectedIndex;
  final ValueChanged<int> onSelected;
  final VoidCallback? onCreate;
  final ValueChanged<MirrorPane>? onClose;

  @override
  Widget build(BuildContext context) {
    return Container(
      height: AppSizes.minTouchTarget,
      color: AppColors.terminalChrome,
      foregroundDecoration: const BoxDecoration(
        border: Border(bottom: BorderSide(color: AppColors.borderDefault)),
      ),
      child: Row(
        children: [
          Expanded(
            child: ListView.builder(
              scrollDirection: Axis.horizontal,
              padding: const EdgeInsets.symmetric(horizontal: AppSizes.p8),
              itemCount: tabs.length,
              itemBuilder: (context, index) => _SurfaceTabButton(
                tab: tabs[index],
                chatLabel: chatLabels[tabs[index].id],
                isSelected: index == selectedIndex,
                onTap: () => onSelected(index),
                onClose: onClose == null ? null : () => onClose!(tabs[index]),
              ),
            ),
          ),
          Semantics(
            identifier: 'create-terminal-tab',
            child: IconButton(
              onPressed: onCreate,
              tooltip: onCreate == null ? '터미널 추가 중…' : '터미널 탭 추가',
              icon: onCreate == null
                  ? const SizedBox.square(
                      dimension: AppSizes.p16,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : const Icon(Icons.add),
            ),
          ),
        ],
      ),
    );
  }
}

class _SurfaceTabButton extends StatelessWidget {
  const _SurfaceTabButton({
    required this.tab,
    this.chatLabel,
    required this.isSelected,
    required this.onTap,
    required this.onClose,
  });

  final MirrorPane tab;
  final String? chatLabel;
  final bool isSelected;
  final VoidCallback onTap;

  /// null이면 닫을 수 없는 탭이다
  final VoidCallback? onClose;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      identifier: 'terminal-tab-${tab.id}',
      selected: isSelected,
      button: true,
      label:
          '${chatLabel ?? (tab.command.isEmpty ? tab.id : tab.command)} ${tab.id} 탭',
      child: InkWell(
        onTap: onTap,
        child: Container(
          width: 160,
          alignment: Alignment.center,
          padding: const EdgeInsets.only(left: AppSizes.p12),
          // 밑줄이 높이를 차지하면 닫기 버튼이 최소 터치 크기보다 작아져 위에 덧그린다
          foregroundDecoration: BoxDecoration(
            border: Border(
              bottom: BorderSide(
                color: isSelected ? AppColors.brandPrimary : Colors.transparent,
                width: 2,
              ),
            ),
          ),
          child: Row(
            children: [
              Expanded(
                child: Column(
                  mainAxisAlignment: MainAxisAlignment.center,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Flexible(
                      child: Tooltip(
                        message: tab.id,
                        child: Text(
                          chatLabel ??
                              (tab.command.isEmpty ? tab.id : tab.command),
                          overflow: TextOverflow.ellipsis,
                          style: monoStyle(
                            fontSize: 13,
                            color: isSelected
                                ? AppColors.textPrimary
                                : chatLabel == null
                                ? AppColors.textMuted
                                : AppColors.textSecondary,
                            fontWeight: isSelected
                                ? FontWeight.w700
                                : FontWeight.w400,
                          ),
                        ),
                      ),
                    ),
                    const SizedBox(height: 2),
                    Text(
                      chatLabel == null
                          ? '${tab.tabName} ${tab.id}'
                          : tab.id.length > 16
                          ? '…${tab.id.substring(tab.id.length - 6)}'
                          : tab.id,
                      overflow: TextOverflow.ellipsis,
                      style: chatLabel == null
                          ? Theme.of(context).textTheme.labelSmall
                          : Theme.of(context).textTheme.labelSmall
                                ?.copyWith(color: AppColors.textSecondary),
                    ),
                  ],
                ),
              ),
              Semantics(
                identifier: 'close-terminal-tab-${tab.id}',
                child: IconButton(
                  onPressed: onClose,
                  tooltip: '탭 닫기',
                  iconSize: AppSizes.p16,
                  constraints: const BoxConstraints.tightFor(
                    width: AppSizes.minTouchTarget,
                    height: AppSizes.minTouchTarget,
                  ),
                  icon: const Icon(Icons.close),
                ),
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
  const _SurfacesUnavailable({
    required this.reason,
    required this.onRetry,
    this.message,
  });

  final SurfaceUnavailableReason reason;
  final VoidCallback onRetry;
  final String? message;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(AppSizes.p24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              message ?? surfaceUnavailableMessage(reason),
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
