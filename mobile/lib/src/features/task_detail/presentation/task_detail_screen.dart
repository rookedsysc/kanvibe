import 'dart:async';

import '../../ai_sessions/presentation/live_ai_sessions_screen.dart';
import '../../diff/presentation/diff_screen.dart';
import 'widgets/multi_pane_terminal.dart';
import 'widgets/terminal_management_sheet.dart';
import '../application/task_details_controller.dart';
import 'task_editor_screen.dart';
import 'related_tasks_screen.dart';
import '../../../common/l10n/app_localizations.dart';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../../../common/constants/app_theme.dart';
import '../../../common/network/desktop_command.dart';
import '../../../common/providers.dart';
import '../../board/application/task_deletion.dart';
import '../../board/domain/board_task.dart';
import '../../board/domain/task_status.dart';
import '../../chat_history/presentation/chat_history_screen.dart';
import '../application/task_mutation.dart';
import '../../notifications/application/mobile_notification_controller.dart';
import '../../pane_chat/application/pane_agent_watcher.dart';
import '../../pane_chat/domain/pane_chat.dart';
import '../../notifications/application/notification_inbox_controller.dart';
import '../../pane_chat/presentation/pane_chat_view.dart';
import '../../task_create/domain/session_type.dart';
import '../../task_create/presentation/create_task_screen.dart';
import '../domain/mirror_pane.dart';
import 'pane_chat_shortcut.dart';
import 'surface_unavailable_message.dart';
import 'widgets/task_info_sheet.dart';
import 'widgets/task_session_type_sheet.dart';
import 'widgets/task_status_sheet.dart';
import 'widgets/terminal_chrome_header.dart';

/// 프로젝트·브랜치와 태스크 동작을 상단에 모으고 선택한 pane을 표시한다.
class TaskDetailScreen extends ConsumerStatefulWidget {
  const TaskDetailScreen({
    required this.task,
    this.embedded = false,
    this.onTaskDeleted,
    this.initialWindowId,
    super.key,
  });

  /// 보드에서 고른 태스크. 제목뿐 아니라 정보 시트가 그리는 칸까지 이 안에 들어 있다
  final BoardTask task;
  final String? initialWindowId;
  final bool embedded;
  final VoidCallback? onTaskDeleted;

  @override
  ConsumerState<TaskDetailScreen> createState() => _TaskDetailScreenState();
}

class _TaskDetailScreenState extends ConsumerState<TaskDetailScreen>
    with WidgetsBindingObserver {
  String? _selectedPaneId;
  final _chatModePaneIds = <String>{};
  final _chatPaneIds = <String>{};
  bool _didSelectInitialWindow = false;

  /// 화면이 들고 있는 태스크. 상태를 옮기면 보드를 다시 받기 전에 여기부터 갈아 끼운다
  late BoardTask _task = widget.task;

  bool _isPreparingCreateTask = false;

  /// 닫고 있는 탭과 지금 단계의 문구. 확인 창으로 답을 기다리는 동안은 문구가 없다.
  /// 이 값이 있는 동안 다른 닫기와 추가는 받지 않는다
  ({String paneId, String? progressLabel})? _closingTab;

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
  void _markTaskNotificationsReadWhenViewing({
    bool refreshVisibleNotifications = false,
  }) {
    final isViewingTask =
        _isAppResumed && (ModalRoute.isCurrentOf(context) ?? true);
    if (isViewingTask && (!_wasViewingTask || refreshVisibleNotifications)) {
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
          SnackBar(
            content: Text(context.tr('프로젝트를 불러오지 못했습니다. 잠시 뒤 다시 시도해 주세요.')),
          ),
        );
        return;
      }
      final snapshot = board.requireValue;
      final project = snapshot.creatableProjects
          .where((project) => project.id == _task.projectId)
          .firstOrNull;
      if (project == null) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(
              context.tr('이 태스크의 원본 프로젝트를 찾을 수 없습니다. 데스크탑에서 프로젝트를 확인해 주세요.'),
            ),
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
      if (widget.embedded) {
        navigator.push<void>(
          MaterialPageRoute(builder: (_) => TaskDetailScreen(task: created)),
        );
      } else {
        navigator.pushReplacement<void, void>(
          MaterialPageRoute(builder: (_) => TaskDetailScreen(task: created)),
        );
      }
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
        SnackBar(
          content: Text(context.tr('터미널 탭을 추가하지 못했습니다. 잠시 뒤 다시 시도해 주세요.')),
        ),
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
    if (!mounted) return false;
    if (processNames.isEmpty) {
      return true;
    }
    return _askCloseConfirmation(
      context.tr('{names} 실행 중인 탭을 닫으면 종료됩니다', {
        'names': processNames.join(', '),
      }),
    );
  }

  Future<bool> _askCloseConfirmation(String message) async {
    if (!mounted) return false;
    // 프로세스 확인은 끝났고 이제 사용자의 답을 기다린다
    setState(
      () => _closingTab = (paneId: _closingTab!.paneId, progressLabel: null),
    );
    final isConfirmed = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        backgroundColor: context.colors.bgSurface,
        title: Text(context.tr('탭을 닫을까요?')),
        content: Text(context.tr(message)),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(dialogContext).pop(false),
            child: Text(context.tr('취소')),
          ),
          TextButton(
            onPressed: () => Navigator.of(dialogContext).pop(true),
            style: TextButton.styleFrom(foregroundColor: AppColors.statusError),
            child: Text(context.tr('닫기')),
          ),
        ],
      ),
    );
    return isConfirmed ?? false;
  }

  /// 누른 탭 하나만 닫는다. 확인하는 동안과 닫는 동안 대상 탭에 진행을 보이고 다른 닫기는 받지 않는다
  Future<void> _closeTab(List<MirrorPane> tabs, MirrorPane tab) async {
    if (_closingTab != null) {
      return;
    }
    setState(() => _closingTab = (paneId: tab.id, progressLabel: '닫기 확인 중…'));
    try {
      if (!await _confirmCloseTab(tab) || !mounted) {
        return;
      }
      setState(() => _closingTab = (paneId: tab.id, progressLabel: '닫는 중…'));
      await _sendCloseTab(tabs, tab);
    } finally {
      if (mounted) {
        setState(() => _closingTab = null);
      }
    }
  }

  /// 선택한 탭을 닫으면 앞 탭으로, 맨 앞이면 다음 탭으로 옮긴다
  Future<void> _sendCloseTab(List<MirrorPane> tabs, MirrorPane tab) async {
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
        .showSnackBar(SnackBar(content: Text(context.tr(message))));
  }

  Future<void> _editTask() async {
    final updated = await Navigator.of(context).push<BoardTask>(
      MaterialPageRoute(builder: (_) => TaskEditorScreen(task: _task)),
    );
    if (mounted && updated != null) setState(() => _task = updated);
  }

  Future<void> _openRelatedTasks() async {
    final projectId = _task.projectId;
    if (projectId == null) return;
    final selected = await Navigator.of(context).push<BoardTask>(
      MaterialPageRoute(
        builder: (_) => RelatedTasksScreen(
          projectId: projectId,
          baseBranch: _task.baseBranch,
        ),
      ),
    );
    if (!mounted || selected == null) return;
    await Navigator.of(context).push(
      MaterialPageRoute<void>(builder: (_) => TaskDetailScreen(task: selected)),
    );
  }

  Future<void> _branchExistingTask() async {
    final result = await AsyncValue.guard(() => ref.read(boardProvider.future));
    if (!mounted) return;
    if (result.hasError) {
      _showNotice(context.tr('프로젝트를 불러오지 못했습니다. 잠시 뒤 다시 시도해 주세요.'));
      return;
    }
    final board = result.requireValue;
    final updated = await Navigator.of(context).push<BoardTask>(
      MaterialPageRoute(
        builder: (_) => CreateTaskScreen(
          projects: board.creatableProjects,
          defaultProjectId: _task.projectId,
          defaultBaseBranch: _task.baseBranch,
          existingTaskId: _task.id,
        ),
      ),
    );
    if (mounted && updated != null) {
      setState(() => _task = updated);
      ref.invalidate(surfacesProvider(_task.id));
    }
  }

  Future<void> _openInfoSheet() => showModalBottomSheet<void>(
    context: context,
    isScrollControlled: true,
    backgroundColor: context.colors.bgSurface,
    builder: (_) => TaskInfoSheet(task: _task),
  );

  /// 옮긴 결과는 시트가 아니라 `taskMutationProvider`에서 받는다. 시트를 먼저 닫아도 반영된다
  Future<void> _openStatusSheet() => showModalBottomSheet<void>(
    context: context,
    backgroundColor: context.colors.bgSurface,
    builder: (_) => TaskStatusSheet(task: _task),
  );

  /// 삭제는 시트가 닫힌 뒤에도 controller에서 끝나므로, 시트의 결과가 아니라 삭제 상태를 보고 보드로 돌아간다
  void _leaveDeletedTask(int failedPeerCount) {
    // 앱 전체의 messenger에 띄워야 보드로 돌아간 뒤에도 경고가 보인다
    if (failedPeerCount > 0) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text(
            context.tr(
              '다른 데스크탑 {count}곳에는 아직 남아 있습니다. 그 데스크탑이 켜지면 직접 삭제해 주세요.',
              {'count': failedPeerCount},
            ),
          ),
        ),
      );
    }
    // 시트가 아직 열려 있으면 함께 닫고, 지운 태스크의 화면에는 비출 pane이 없으니 보드로 돌아간다
    final detailRoute = ModalRoute.of(context);
    Navigator.of(context).popUntil((route) => route == detailRoute);
    if (widget.embedded) {
      widget.onTaskDeleted?.call();
    } else {
      Navigator.of(context).pop();
    }
  }

  Future<void> _openSessionTypeSheet(SessionType currentSessionType) =>
      showModalBottomSheet<SessionType>(
        context: context,
        backgroundColor: context.colors.bgSurface,
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
      final url =
          _task.prUrl ??
          await ref
              .read(taskDetailsProvider(_task.id).notifier)
              .findPullRequest();
      if (!mounted) return;
      if (url == null) {
        messenger.showSnackBar(
          SnackBar(content: Text(context.tr('연결된 PR을 찾지 못했습니다.'))),
        );
        return;
      }
      opened = await launchUrl(
        Uri.parse(url),
        mode: LaunchMode.externalApplication,
      );
    } catch (_) {
      opened = false;
    }

    if (!opened && mounted) {
      messenger.showSnackBar(
        SnackBar(content: Text(context.tr('PR을 열 수 있는 앱이 없습니다.'))),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(taskDeletionProvider.select((progress) => progress[_task.id]), (
      _,
      next,
    ) {
      if (next is TaskDeletionDone) {
        _leaveDeletedTask(next.failedPeerCount);
      }
    });
    ref.listen(taskMutationProvider.select((progress) => progress[_task.id]), (
      _,
      next,
    ) {
      if (next is TaskStatusMoved && next.status != _task.status) {
        setState(() => _task = _task.withStatus(next.status));
      }
    });
    ref.listen(taskNotificationUpdatesProvider(_task.id), (_, next) {
      if (next.hasValue && next.value != null) {
        _markTaskNotificationsReadWhenViewing(
          refreshVisibleNotifications: true,
        );
      }
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
            label: context.tr('대화 기록'),
            icon: Icons.history,
            onPressed: () => Navigator.of(context).push(
              MaterialPageRoute<void>(
                builder: (_) => ChatHistoryScreen(taskId: _task.id),
              ),
            ),
          ),
          (
            id: 'live-ai-sessions',
            label: context.tr('실행 중 AI 세션'),
            icon: Icons.smart_toy_outlined,
            onPressed: () => Navigator.of(context).push(
              MaterialPageRoute<void>(
                builder: (_) => LiveAiSessionsScreen(
                  taskId: _task.id,
                  onSelectWindow: (windowId) {
                    final panes =
                        ref
                            .read(surfacesProvider(_task.id))
                            .value
                            ?.surfaces
                            .allPanes ??
                        [];
                    final pane = panes
                        .where((pane) => pane.tabId == windowId)
                        .firstOrNull;
                    if (pane != null) {
                      setState(() => _selectedPaneId = pane.id);
                      Navigator.of(context).pop();
                    } else {
                      _showNotice(context.tr('해당 터미널 탭을 찾지 못했습니다.'));
                    }
                  },
                ),
              ),
            ),
          ),
          (
            id: 'task-diff',
            label: context.tr('변경 파일'),
            icon: Icons.difference_outlined,
            onPressed: () => Navigator.of(context).push(
              MaterialPageRoute<void>(
                builder: (_) => DiffScreen(taskId: _task.id),
              ),
            ),
          ),
          (
            id: 'manage-terminal',
            label: context.tr('터미널 관리'),
            icon: Icons.tune,
            onPressed: surfaces.hasValue
                ? () => showModalBottomSheet<void>(
                    context: context,
                    isScrollControlled: true,
                    builder: (_) => TerminalManagementSheet(
                      taskId: _task.id,
                      selectedPaneId:
                          _selectedPaneId ??
                          surfaces
                              .requireValue
                              .surfaces
                              .allPanes
                              .firstOrNull
                              ?.id ??
                          '',
                      isRemote: _task.isRemote,
                    ),
                  )
                : null,
          ),
          (
            id: 'edit-task',
            label: context.tr('정보 수정'),
            icon: Icons.edit_outlined,
            onPressed: _editTask,
          ),
          if (_task.projectId != null)
            (
              id: 'related-tasks',
              label: context.tr('프로젝트 작업'),
              icon: Icons.account_tree_outlined,
              onPressed: _openRelatedTasks,
            ),
          if (_task.branchName == null)
            (
              id: 'branch-existing-task',
              label: context.tr('브랜치 작업으로 전환'),
              icon: Icons.fork_right,
              onPressed: _branchExistingTask,
            ),
          (
            id: 'task-info',
            label: context.tr('정보'),
            icon: Icons.info_outline,
            onPressed: _openInfoSheet,
          ),
          (
            id: 'task-status',
            label: context.tr('상태 옮기기'),
            icon: Icons.swap_horiz,
            onPressed: _openStatusSheet,
          ),
          (
            id: 'change-session-type',
            label: context.tr('세션 종류 바꾸기'),
            icon: Icons.terminal,
            onPressed: surfaces.hasValue
                ? () => _openSessionTypeSheet(
                    surfaces.requireValue.surfaces.sessionType,
                  )
                : null,
          ),
          if (_task.prUrl != null || _task.branchName != null)
            (
              id: 'task-pull-request',
              label: context.tr('PR 열기'),
              icon: Icons.call_merge,
              onPressed: _openPullRequest,
            ),
        ];

    final content = LayoutBuilder(
      builder: (context, constraints) {
        final isCompact = !isTabletLayout(constraints);
        return Scaffold(
          appBar: AppBar(
            automaticallyImplyLeading: !widget.embedded,
            titleSpacing: AppSizes.p8,
            leadingWidth: AppSizes.minTouchTarget,
            title: TerminalChromeHeader(task: _task),
            actions: [
              if (isCompact)
                Semantics(
                  identifier: 'task-more',
                  child: PopupMenuButton<VoidCallback>(
                    tooltip: context.tr('태스크 메뉴'),
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
                                  SizedBox(width: AppSizes.p12),
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
                    loading: () => Center(child: CircularProgressIndicator()),
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
                      isCreating: value.isCreating,
                      isMutating: value.isMutating,
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

  Widget _buildSurfaces(
    TaskSurfaces surfaces, {
    required bool isCreating,
    required bool isMutating,
  }) {
    final tabs = surfaces.allPanes;

    if (tabs.isEmpty) {
      return _SurfacesUnavailable(
        reason: SurfaceUnavailableReason.sessionNotRunning,
        onRetry: () =>
            ref.read(surfacesProvider(_task.id).notifier).startSession(),
      );
    }

    if (!_didSelectInitialWindow && widget.initialWindowId != null) {
      _selectedPaneId = tabs
          .where((pane) => pane.tabId == widget.initialWindowId)
          .firstOrNull
          ?.id;
      _didSelectInitialWindow = true;
    }
    final matchingIndex = tabs.indexWhere((pane) => pane.id == _selectedPaneId);
    final selectedIndex = matchingIndex < 0 ? 0 : matchingIndex;

    return Column(
      children: [
        _SurfaceTabBar(
          tabs: tabs,
          selectedIndex: selectedIndex,
          onSelected: (index) => setState(() {
            _selectedPaneId = tabs[index].id;
          }),
          isCreating: isCreating,
          closingTab: _closingTab,
          onCreate: isMutating || _closingTab != null
              ? null
              : _createTerminalTab,
          // 탭이 하나뿐이면 세션이 끝나 버리므로 닫을 수 없다
          onClose: isMutating || _closingTab != null || tabs.length == 1
              ? null
              : (tab) => _closeTab(tabs, tab),
        ),
        Expanded(child: _buildSelectedSurface(tabs[selectedIndex], surfaces)),
      ],
    );
  }

  /// 세션 종류가 바뀌면 pane id가 같더라도 다른 세션이므로 pane 화면을 새로 만들어 스트림을 다시 연다
  Widget _buildSelectedSurface(MirrorPane pane, TaskSurfaces surfaces) {
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
              child: MultiPaneTerminal(
                taskId: _task.id,
                surfaces: surfaces,
                selectedPane: pane,
                onSelectedPane: (id) => setState(() => _selectedPaneId = id),
                isActive: !chatMode && routeActive,
                onToggleChat: _toggleSelectedPaneChat,
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
                    isActive: chatMode && chatPane.id == pane.id && routeActive,
                    onReturnToTerminal: () =>
                        _setPaneChatMode(chatPane.id, false),
                  ),
                ),
              ),
            ),
          Positioned(
            top: AppSizes.p12,
            right: AppSizes.p12,
            child: _PaneChatToggle(
              target: (taskId: _task.id, paneId: pane.id),
              isChatMode: chatMode,
              onPressed: _toggleSelectedPaneChat,
            ),
          ),
        ],
      ),
    );
  }
}

/// 터미널 위에 떠서 선택한 pane을 채팅으로 볼지 정한다. 켜짐과 꺼짐은 테두리 색으로만 구분한다
class _PaneChatToggle extends ConsumerWidget {
  const _PaneChatToggle({
    required this.target,
    required this.isChatMode,
    required this.onPressed,
  });

  final PaneChatTarget target;
  final bool isChatMode;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    // 채팅을 보는 동안에는 채팅 화면이 같은 조회를 하므로 따로 확인하지 않는다
    final hasAgent =
        !isChatMode && ref.watch(paneAgentProvider(target)) != null;
    if (!isChatMode && !hasAgent) return const SizedBox.shrink();
    return Semantics(
      identifier: 'pane-chat-toggle',
      toggled: isChatMode,
      child: OutlinedButton.icon(
        onPressed: onPressed,
        icon: const Icon(Icons.forum_outlined, size: AppSizes.p16),
        label: Text(context.tr('채팅')),
        style: OutlinedButton.styleFrom(
          foregroundColor: context.colors.textPrimary,
          backgroundColor: context.colors.buttonNeutral,
          minimumSize: const Size(0, AppSizes.minTouchTarget),
          padding: const EdgeInsets.symmetric(horizontal: AppSizes.p12),
          side: BorderSide(
            color: isChatMode ? AppColors.brandPrimary : context.colors.borderStrong,
          ),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(AppSizes.radiusCard),
          ),
        ),
      ),
    );
  }
}

class _SurfaceTabBar extends StatelessWidget {
  const _SurfaceTabBar({
    required this.tabs,
    required this.selectedIndex,
    required this.onSelected,
    required this.isCreating,
    required this.closingTab,
    required this.onCreate,
    required this.onClose,
  });

  final List<MirrorPane> tabs;
  final int selectedIndex;
  final ValueChanged<int> onSelected;

  /// 탭을 추가하는 중일 때만 추가 버튼에 진행을 보인다. 닫기 같은 다른 요청 중에는 버튼만 막는다
  final bool isCreating;
  final ({String paneId, String? progressLabel})? closingTab;
  final VoidCallback? onCreate;
  final ValueChanged<MirrorPane>? onClose;

  @override
  Widget build(BuildContext context) {
    return Container(
      height: AppSizes.minTouchTarget,
      color: AppColors.terminalChrome,
      foregroundDecoration: BoxDecoration(
        border: Border(bottom: BorderSide(color: AppColors.borderDefault)),
      ),
      child: Row(
        children: [
          Expanded(
            child: ListView.builder(
              scrollDirection: Axis.horizontal,
              padding: EdgeInsets.symmetric(horizontal: AppSizes.p8),
              itemCount: tabs.length,
              itemBuilder: (context, index) => _SurfaceTabButton(
                tab: tabs[index],
                isSelected: index == selectedIndex,
                onTap: () => onSelected(index),
                onClose: onClose == null ? null : () => onClose!(tabs[index]),
                closingLabel: closingTab?.paneId == tabs[index].id
                    ? closingTab!.progressLabel
                    : null,
              ),
            ),
          ),
          Semantics(
            identifier: 'create-terminal-tab',
            child: IconButton(
              onPressed: onCreate,
              tooltip: context.tr(isCreating ? '터미널 추가 중…' : '터미널 탭 추가'),
              icon: isCreating
                  ? const SizedBox.square(
                      dimension: AppSizes.p16,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : Icon(Icons.add, color: AppColors.textSecondary),
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
    required this.isSelected,
    required this.onTap,
    required this.onClose,
    this.closingLabel,
  });

  final MirrorPane tab;
  final bool isSelected;
  final VoidCallback onTap;

  /// null이면 닫을 수 없는 탭이다
  final VoidCallback? onClose;

  /// 이 탭을 닫는 중이면 그 단계 문구. 탭 이름 아래 줄 대신 보인다
  final String? closingLabel;

  @override
  Widget build(BuildContext context) {
    final name = [
      tab.command.trim(),
      tab.tabName.trim(),
      'pane',
    ].firstWhere((value) => value.isNotEmpty);
    return Semantics(
      identifier: 'terminal-tab-${tab.id}',
      selected: isSelected,
      button: true,
      label: context.tr('{name} {id} 탭', {'name': name, 'id': tab.id}),
      child: InkWell(
        onTap: onTap,
        child: Container(
          width: 160,
          alignment: Alignment.center,
          padding: EdgeInsets.only(left: AppSizes.p12),
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
                          name,
                          overflow: TextOverflow.ellipsis,
                          style: monoStyle(
                            fontSize: 13,
                            color: isSelected
                                ? AppColors.textPrimary
                                : AppColors.textSecondary,
                            fontWeight: isSelected
                                ? FontWeight.w700
                                : FontWeight.w400,
                          ),
                        ),
                      ),
                    ),
                    // 평소에는 이름만 보이고, 닫는 중일 때만 그 단계 문구를 아래 줄에 보인다
                    if (closingLabel != null) ...[
                      const SizedBox(height: 2),
                      Text(
                        closingLabel!,
                        overflow: TextOverflow.ellipsis,
                        style: Theme.of(context).textTheme.labelSmall
                            ?.copyWith(color: AppColors.textSecondary),
                      ),
                    ],
                  ],
                ),
              ),
              Semantics(
                identifier: 'close-terminal-tab-${tab.id}',
                child: IconButton(
                  onPressed: onClose,
                  tooltip: context.tr('탭 닫기'),
                  iconSize: AppSizes.p16,
                  constraints: BoxConstraints.tightFor(
                    width: AppSizes.minTouchTarget,
                    height: AppSizes.minTouchTarget,
                  ),
                  icon: closingLabel == null
                      ? Icon(
                          Icons.close,
                          color: onClose == null
                              ? AppColors.textMuted
                              : AppColors.textSecondary,
                        )
                      : const SizedBox.square(
                          dimension: AppSizes.p12,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        ),
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
        padding: EdgeInsets.all(AppSizes.p24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              message ?? surfaceUnavailableMessage(reason),
              textAlign: TextAlign.center,
              style: Theme.of(context).textTheme.bodyMedium,
            ),
            SizedBox(height: AppSizes.p16),
            FilledButton(onPressed: onRetry, child: Text(context.tr('다시 시도'))),
          ],
        ),
      ),
    );
  }
}
