import '../../settings/presentation/pc_settings_screen.dart';
import '../../ai_sessions/presentation/widgets/running_ai_badge.dart';
import '../../ai_sessions/presentation/live_ai_sessions_screen.dart';
import '../../ai_accounts/presentation/ai_accounts_screen.dart';
import '../../ai_accounts/presentation/ai_usage_screen.dart';
import '../../diff/presentation/widgets/task_diff_summary.dart';
import '../../pane_layouts/presentation/pane_layouts_screen.dart';

import 'dart:async';

import 'package:url_launcher/url_launcher.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/project_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../../../common/constants/app_theme.dart';
import '../../../common/providers.dart';
import '../../../common/l10n/app_localizations.dart';
import '../../settings/presentation/device_preferences_screen.dart';
import '../application/board_sort_controller.dart';
import '../application/board_pagination_controller.dart';
import '../domain/board_view_options.dart';
import '../domain/task_priority.dart';
import 'task_quick_search_screen.dart';
import 'widgets/board_sort_sheet.dart';
import 'widgets/done_pagination_control.dart';
import 'widgets/task_project_icon.dart';
import '../../task_create/presentation/create_task_screen.dart';
import '../../project_registry/presentation/project_registry_screen.dart';
import '../../task_detail/presentation/task_detail_screen.dart';
import '../../connection/presentation/desktop_connections_screen.dart';
import '../../notifications/presentation/notification_center_screen.dart';
import '../../notifications/application/notification_inbox_controller.dart';
import '../application/board_project_filter_controller.dart';
import '../application/task_deletion.dart';
import '../data/board_snapshot.dart';
import '../domain/board_task.dart';
import '../domain/task_status.dart';
import 'widgets/project_filter_button.dart';
import 'widgets/desktop_filter_button.dart';
import 'widgets/task_delete_dialogs.dart';

/// 첫 화면. 태스크를 상태별로 나눠 보여 준다.
///
/// 폰과 태블릿은 같은 데이터를 다르게 놓는다. 좁은 화면에서 칸반을 옆으로 밀어 보게 하면
/// 한 번에 한 칸밖에 못 보므로, 폰은 상태를 세로 구획으로 쌓고 태블릿만 칸반으로 편다.
/// 갈림길은 [isTabletLayout] 하나뿐이고 데이터는 한 번만 불러온다.
class BoardScreen extends ConsumerStatefulWidget {
  const BoardScreen({super.key});
  @override
  ConsumerState<BoardScreen> createState() => _BoardScreenState();
}

class _BoardScreenState extends ConsumerState<BoardScreen> {
  final _findController = TextEditingController();
  final _cardKeys = <String, GlobalKey>{};
  bool _isFinding = false;
  bool _isWorkspace = false;
  String _findQuery = '';
  int _matchIndex = 0;
  String? _selectedTaskId;
  BoardTask? _selectedTask;

  @override
  void dispose() {
    _findController.dispose();
    super.dispose();
  }

  List<BoardTask> _matches(BoardSnapshot snapshot) => _findQuery.trim().isEmpty
      ? []
      : snapshot.allTasks
            .where(
              (task) =>
                  [
                        task.title,
                        task.projectName ?? '',
                        task.branchName ?? '',
                        task.description ?? '',
                        task.agentType ?? '',
                        task.sessionType ?? '',
                        task.sshHost ?? '',
                        task.status.label,
                      ]
                      .join(' ')
                      .toLowerCase()
                      .contains(_findQuery.trim().toLowerCase()),
            )
            .toList();

  void _findNext(BoardSnapshot snapshot, int delta) {
    final matches = _matches(snapshot);
    if (matches.isEmpty) return;
    setState(() => _matchIndex = (_matchIndex + delta) % matches.length);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      final target =
          _cardKeys[matches[_matchIndex % matches.length].id]?.currentContext;
      if (mounted && target != null) {
        Scrollable.ensureVisible(
          target,
          duration: const Duration(milliseconds: 250),
          alignment: 0.25,
        );
      }
    });
  }

  void _openTask(BoardTask task) {
    if (_isWorkspace &&
        MediaQuery.sizeOf(context).width >= AppSizes.tabletBreakpoint) {
      setState(() {
        _selectedTaskId = task.id;
        _selectedTask = task;
      });
      return;
    }
    Navigator.of(context).push(
      MaterialPageRoute<void>(builder: (_) => TaskDetailScreen(task: task)),
    );
  }

  Future<void> _searchTasks() async {
    final task = await Navigator.of(context).push<BoardTask>(
      MaterialPageRoute(builder: (_) => const TaskQuickSearchScreen()),
    );
    if (mounted && task != null) _openTask(task);
  }

  Widget _card(BoardTask task, BoardSnapshot snapshot) {
    final matches = _matches(snapshot);
    final found =
        matches.isNotEmpty &&
        matches[_matchIndex % matches.length].id == task.id;
    final complete =
        ref.read(boardPaginationProvider).value?.snapshot ?? snapshot;
    final priority = effectivePriority(
      task,
      complete.allTasks,
      complete.projects,
    );
    return KeyedSubtree(
      key: _cardKeys.putIfAbsent(task.id, GlobalKey.new),
      child: _TaskCard(
        task: task,
        onOpen: () => _openTask(task),
        isFindMatch: found,
        isSelected: _isWorkspace && _selectedTaskId == task.id,
        displayPriority: priority,
        isPriorityInherited: task.priority == null && priority != null,
        isBaseProject: isProjectRoot(task, snapshot.projects),
      ),
    );
  }

  Widget _workspace(BoardSnapshot snapshot, String connectionId) {
    final selected =
        snapshot.allTasks
            .where((task) => task.id == _selectedTaskId)
            .firstOrNull ??
        _selectedTask;
    return Row(
      children: [
        SizedBox(
          width: 300,
          child: SingleChildScrollView(
            padding: const EdgeInsets.fromLTRB(
              AppSizes.p12,
              AppSizes.p12,
              AppSizes.p12,
              AppSizes.minTouchTarget + AppSizes.p32,
            ),
            child: Column(
              children: [
                for (final task in snapshot.allTasks)
                  Padding(
                    padding: const EdgeInsets.only(bottom: AppSizes.p8),
                    child: _card(task, snapshot),
                  ),
                const DonePaginationControl(),
              ],
            ),
          ),
        ),
        VerticalDivider(width: 1, color: context.colors.borderDefault),
        Expanded(
          child: selected == null
              ? Center(child: Text(context.tr('태스크를 선택해 주세요.')))
              : TaskDetailScreen(
                  key: ValueKey('$connectionId/${selected.id}'),
                  task: selected,
                  embedded: true,
                  onTaskDeleted: () => setState(() {
                    _selectedTaskId = null;
                    _selectedTask = null;
                  }),
                ),
        ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final board = ref.watch(boardProvider);
    final filteredBoard = ref.watch(filteredBoardProvider);
    final connection = ref
        .watch(connectionControllerProvider)
        .value
        ?.activeConnection;
    final workspaceWidth =
        MediaQuery.sizeOf(context).width -
        MediaQuery.paddingOf(context).horizontal;
    final workspaceVisible =
        _isWorkspace && workspaceWidth >= AppSizes.tabletBreakpoint;
    ref.listen(
      connectionControllerProvider.select((value) => value.value?.activeId),
      (previous, next) {
        if (previous != next) {
          _selectedTaskId = null;
          _selectedTask = null;
          _findQuery = '';
          _findController.clear();
          _matchIndex = 0;
          _cardKeys.clear();
        }
      },
    );
    return Scaffold(
      appBar: AppBar(
        title: TextButton(
          onPressed: () => Navigator.of(context).push(
            MaterialPageRoute<void>(
              builder: (_) => const DesktopConnectionsScreen(),
            ),
          ),
          style: TextButton.styleFrom(
            foregroundColor: context.colors.textPrimary,
            alignment: Alignment.centerLeft,
            padding: const EdgeInsets.symmetric(horizontal: AppSizes.p8),
            minimumSize: const Size(48, 48),
          ),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Flexible(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(
                      connection?.displayName ?? 'KanVibe',
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                    if (connection != null)
                      Text(
                        connection.address,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: Theme.of(context).textTheme.labelSmall,
                      ),
                  ],
                ),
              ),
              const SizedBox(width: AppSizes.p4),
              const Icon(Icons.keyboard_arrow_down, size: 20),
            ],
          ),
        ),
        actions: [
          const NotificationCenterButton(),
          Semantics(
            identifier: 'board-more',
            child: PopupMenuButton<String>(
              key: const ValueKey('board-more'),
              tooltip: context.tr('보드 메뉴'),
              onSelected: (action) {
                if (action == 'refresh') ref.invalidate(boardProvider);
                final page = switch (action) {
                  'pc-settings' => const PcSettingsScreen(),
                  'ai-accounts' => const AiAccountsScreen(),
                  'ai-usage' => const AiUsageScreen(),
                  'pane-layouts' => const PaneLayoutsScreen(),
                  _ => null,
                };
                if (page != null) {
                  Navigator.of(context)
                      .push(MaterialPageRoute<void>(builder: (_) => page));
                }
                if (action == 'preferences') {
                  Navigator.of(context).push(
                    MaterialPageRoute<void>(
                      builder: (_) => const DevicePreferencesScreen(),
                    ),
                  );
                }
                if (action == 'register' && connection != null) {
                  Navigator.of(context).push(
                    MaterialPageRoute<void>(
                      builder: (_) =>
                          ProjectRegistryScreen(connection: connection),
                    ),
                  );
                }
              },
              itemBuilder: (_) => [
                PopupMenuItem(
                  value: 'pc-settings',
                  child: Text(context.tr('PC 설정')),
                ),
                PopupMenuItem(
                  value: 'ai-accounts',
                  child: Text(context.tr('AI 계정')),
                ),
                PopupMenuItem(
                  value: 'ai-usage',
                  child: Text(context.tr('AI 사용량')),
                ),
                PopupMenuItem(
                  value: 'pane-layouts',
                  child: Text(context.tr('Pane 레이아웃')),
                ),
                PopupMenuItem(
                  value: 'register',
                  enabled: connection != null,
                  child: Text(context.tr('프로젝트 등록')),
                ),
                PopupMenuItem(
                  value: 'refresh',
                  child: Text(context.tr('새로고침')),
                ),
                PopupMenuItem(
                  value: 'preferences',
                  child: Text(context.tr('기기 설정')),
                ),
              ],
            ),
          ),
        ],
      ),
      floatingActionButtonLocation: workspaceVisible
          ? FloatingActionButtonLocation.startFloat
          : FloatingActionButtonLocation.endFloat,
      floatingActionButton: board.value == null
          ? null
          : FloatingActionButton(
              onPressed: () => _createTask(context, board.requireValue),
              tooltip: context.tr('새 태스크'),
              child: const Icon(Icons.add),
            ),
      body: SafeArea(
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: AppSizes.p12),
              child: Row(
                children: [
                  Semantics(
                    identifier: 'board-global-search',
                    child: IconButton(
                      tooltip: context.tr('전체 태스크 검색'),
                      onPressed: connection == null ? null : _searchTasks,
                      icon: const Icon(Icons.search),
                    ),
                  ),
                  Semantics(
                    identifier: 'board-find',
                    child: IconButton(
                      tooltip: context.tr('현재 보드에서 찾기'),
                      onPressed: () => setState(() => _isFinding = !_isFinding),
                      isSelected: _isFinding,
                      icon: const Icon(Icons.find_in_page_outlined),
                    ),
                  ),
                  Semantics(
                    identifier: 'board-sort',
                    child: IconButton(
                      tooltip: context.tr('보드 정렬'),
                      onPressed: connection == null
                          ? null
                          : () => showModalBottomSheet<void>(
                              context: context,
                              isScrollControlled: true,
                              useSafeArea: true,
                              builder: (_) => const BoardSortSheet(),
                            ),
                      icon: const Icon(Icons.sort),
                    ),
                  ),
                  Expanded(
                    child: DropdownButtonHideUnderline(
                      child: DropdownButton<BoardTaskKind>(
                        isExpanded: true,
                        value: ref.watch(boardTaskKindProvider),
                        items: [
                          for (final kind in BoardTaskKind.values)
                            DropdownMenuItem(
                              value: kind,
                              child: Text(
                                context.tr(switch (kind) {
                                  BoardTaskKind.all => '전체',
                                  BoardTaskKind.project => '프로젝트 본체',
                                  BoardTaskKind.task => '일반 태스크',
                                }),
                                overflow: TextOverflow.ellipsis,
                              ),
                            ),
                        ],
                        onChanged: (kind) {
                          if (kind != null) {
                            ref
                                .read(boardTaskKindProvider.notifier)
                                .select(kind);
                          }
                        },
                      ),
                    ),
                  ),
                  const DesktopFilterButton(),
                  const ProjectFilterButton(),
                  if (MediaQuery.sizeOf(context).width >=
                      AppSizes.tabletBreakpoint)
                    IconButton(
                      tooltip: context.tr(
                        _isWorkspace ? '칸반 보기' : 'Workspace 보기',
                      ),
                      isSelected: _isWorkspace,
                      onPressed: () =>
                          setState(() => _isWorkspace = !_isWorkspace),
                      icon: const Icon(Icons.view_sidebar_outlined),
                    ),
                ],
              ),
            ),
            if (_isFinding)
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: AppSizes.p12),
                child: Row(
                  children: [
                    Expanded(
                      child: TextField(
                        controller: _findController,
                        decoration: InputDecoration(
                          labelText: context.tr('현재 보드에서 찾기'),
                        ),
                        onChanged: (query) {
                          setState(() {
                            _findQuery = query;
                            _matchIndex = 0;
                          });
                          if (filteredBoard.value != null) {
                            _findNext(filteredBoard.requireValue, 0);
                          }
                        },
                      ),
                    ),
                    Text(
                      '${_matches(filteredBoard.value ?? const BoardSnapshot(tasksByStatus: {})).isEmpty ? 0 : _matchIndex + 1}/${_matches(filteredBoard.value ?? const BoardSnapshot(tasksByStatus: {})).length}',
                    ),
                    IconButton(
                      tooltip: context.tr('이전 결과'),
                      onPressed: filteredBoard.value == null
                          ? null
                          : () => _findNext(filteredBoard.requireValue, -1),
                      icon: const Icon(Icons.keyboard_arrow_up),
                    ),
                    IconButton(
                      tooltip: context.tr('다음 결과'),
                      onPressed: filteredBoard.value == null
                          ? null
                          : () => _findNext(filteredBoard.requireValue, 1),
                      icon: const Icon(Icons.keyboard_arrow_down),
                    ),
                  ],
                ),
              ),
            Expanded(
              child: filteredBoard.when(
                loading: () => const Center(child: CircularProgressIndicator()),
                error: (_, _) => _BoardLoadFailure(
                  onRetry: () => ref.invalidate(boardProvider),
                ),
                data: (snapshot) => RefreshIndicator(
                  onRefresh: () async {
                    ref.invalidate(boardProvider);
                    await ref.read(boardProvider.future);
                  },
                  child: LayoutBuilder(
                    builder: (context, constraints) =>
                        _isWorkspace && isTabletLayout(constraints)
                        ? _workspace(snapshot, connection?.id ?? '')
                        : isTabletLayout(constraints)
                        ? _KanbanBoard(
                            snapshot: snapshot,
                            cardBuilder: (task) => _card(task, snapshot),
                          )
                        : _StatusListView(
                            snapshot: snapshot,
                            cardBuilder: (task) => _card(task, snapshot),
                          ),
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// 새 태스크를 만들고 곧바로 그 태스크로 들어간다.
///
/// 데스크탑도 생성 창을 닫은 뒤 만들어진 태스크의 화면을 연다. 만들자마자 터미널을 보려는 흐름이라
/// 보드로 돌려보내고 카드를 다시 찾게 하면 한 단계가 더 든다.
Future<void> _createTask(BuildContext context, BoardSnapshot board) async {
  final navigator = Navigator.of(context);
  final created = await navigator.push<BoardTask>(
    MaterialPageRoute(
      builder: (_) => CreateTaskScreen(projects: board.creatableProjects),
    ),
  );

  if (created == null) {
    return;
  }

  await navigator.push(
    MaterialPageRoute<void>(builder: (_) => TaskDetailScreen(task: created)),
  );
}

/// 태블릿 배치. 상태별 칸을 옆으로 늘어놓는다
class _KanbanBoard extends StatelessWidget {
  const _KanbanBoard({required this.snapshot, required this.cardBuilder});
  final BoardSnapshot snapshot;
  final Widget Function(BoardTask) cardBuilder;
  static const _columnWidth = 320.0;

  @override
  Widget build(BuildContext context) {
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      padding: const EdgeInsets.all(AppSizes.p16),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          for (final status in TaskStatus.values)
            Padding(
              padding: const EdgeInsets.only(right: AppSizes.p12),
              child: SizedBox(
                width: _columnWidth,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    _StatusHeading(
                      status: status,
                      count: snapshot.tasksIn(status).length,
                    ),
                    const SizedBox(height: AppSizes.p8),
                    Expanded(
                      child: SingleChildScrollView(
                        child: Column(
                          children: [
                            if (snapshot.tasksIn(status).isEmpty)
                              const _EmptyStatus(),
                            for (final task in snapshot.tasksIn(status))
                              Padding(
                                padding: const EdgeInsets.only(
                                  bottom: AppSizes.p8,
                                ),
                                child: cardBuilder(task),
                              ),
                            if (status == TaskStatus.done)
                              const DonePaginationControl(),
                          ],
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ),
        ],
      ),
    );
  }
}

/// 폰 배치. 상태를 세로로 쌓고 머리글을 화면 위에 붙여 둔다
class _StatusListView extends StatelessWidget {
  const _StatusListView({required this.snapshot, required this.cardBuilder});
  final BoardSnapshot snapshot;
  final Widget Function(BoardTask) cardBuilder;
  @override
  Widget build(BuildContext context) => CustomScrollView(
    slivers: [
      if (snapshot.isEmpty)
        SliverToBoxAdapter(
          child: Padding(
            padding: const EdgeInsets.all(AppSizes.p16),
            child: Text(context.tr('아직 태스크가 없습니다.')),
          ),
        ),
      for (final status in TaskStatus.values) ...[
        SliverPersistentHeader(
          pinned: true,
          delegate: _StatusHeaderDelegate(
            status: status,
            count: snapshot.tasksIn(status).length,
          ),
        ),
        SliverPadding(
          padding: const EdgeInsets.fromLTRB(
            AppSizes.p16,
            AppSizes.p8,
            AppSizes.p16,
            AppSizes.p16,
          ),
          sliver: SliverToBoxAdapter(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                if (snapshot.tasksIn(status).isEmpty) const _EmptyStatus(),
                for (final task in snapshot.tasksIn(status))
                  Padding(
                    padding: const EdgeInsets.only(bottom: AppSizes.p8),
                    child: cardBuilder(task),
                  ),
                if (status == TaskStatus.done) const DonePaginationControl(),
              ],
            ),
          ),
        ),
      ],
    ],
  );
}

/// 폰에서 스크롤해도 지금 보고 있는 구획이 무엇인지 남게 한다
class _StatusHeaderDelegate extends SliverPersistentHeaderDelegate {
  const _StatusHeaderDelegate({required this.status, required this.count});

  final TaskStatus status;
  final int count;

  static const _height = 40.0;

  @override
  double get minExtent => _height;

  @override
  double get maxExtent => _height;

  @override
  Widget build(
    BuildContext context,
    double shrinkOffset,
    bool overlapsContent,
  ) {
    return Container(
      height: _height,
      color: context.colors.bgPage,
      padding: const EdgeInsets.symmetric(horizontal: AppSizes.p16),
      alignment: Alignment.centerLeft,
      child: _StatusHeading(status: status, count: count),
    );
  }

  @override
  bool shouldRebuild(_StatusHeaderDelegate oldDelegate) =>
      oldDelegate.status != status || oldDelegate.count != count;
}

/// 상태 이름과 개수. 색은 상태를 나타내는 유일한 수단이 아니라 이름 옆의 보조 표시다
class _StatusHeading extends StatelessWidget {
  const _StatusHeading({required this.status, required this.count});

  final TaskStatus status;
  final int count;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Container(
          width: AppSizes.p8,
          height: AppSizes.p8,
          decoration: BoxDecoration(
            color: status.color,
            shape: BoxShape.circle,
          ),
        ),
        const SizedBox(width: AppSizes.p8),
        Text(status.label, style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(width: AppSizes.p8),
        Text('$count', style: Theme.of(context).textTheme.labelSmall),
      ],
    );
  }
}

class _TaskCard extends ConsumerWidget {
  const _TaskCard({
    required this.task,
    required this.onOpen,
    this.isFindMatch = false,
    this.isSelected = false,
    this.displayPriority,
    this.isPriorityInherited = false,
    this.isBaseProject = false,
  });
  final VoidCallback onOpen;
  final bool isFindMatch;
  final bool isSelected;
  final TaskPriority? displayPriority;
  final bool isPriorityInherited;
  final bool isBaseProject;

  Future<void> _openPr(BuildContext context) async {
    final url = task.prUrl;
    if (url == null) return;
    var opened = false;
    try {
      opened = await launchUrl(
        Uri.parse(url),
        mode: LaunchMode.externalApplication,
      );
    } catch (_) {
      opened = false;
    }
    if (!opened && context.mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text(context.tr('PR을 열 수 있는 앱이 없습니다.'))),
      );
    }
  }

  final BoardTask task;

  /// 길게 눌러 지운다. 커밋하지 않은 변경이 있으면 상세 화면의 삭제처럼 강제 삭제를 한 번 더 묻는다
  Future<void> _deleteTask(BuildContext context, WidgetRef ref) async {
    final connection = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    final client = ref.read(desktopClientProvider);
    final container = ProviderScope.containerOf(context, listen: false);
    final messenger = ScaffoldMessenger.of(context);
    if (connection == null ||
        !await confirmTaskDeletion(context) ||
        !context.mounted) {
      return;
    }

    var outcome = await requestTaskDeletion(
      client,
      connection,
      task.id,
      isForced: false,
    );
    if (outcome is TaskDeletionNeedsForce) {
      if (context.mounted &&
          await confirmForcedTaskDeletion(context) &&
          context.mounted) {
        outcome = await requestTaskDeletion(
          client,
          connection,
          task.id,
          isForced: true,
        );
      } else {
        return;
      }
    }

    if (outcome is TaskDeletionNeedsRegistrationRemoval) {
      if (!context.mounted ||
          !await confirmTaskRegistrationRemoval(context) ||
          !context.mounted) {
        return;
      }
      outcome = await requestTaskDeletion(
        client,
        connection,
        task.id,
        isForced: false,
        isRegistrationOnly: true,
      );
    }

    // 새로고침으로 카드가 사라져도 앱 범위의 보드 갱신과 삭제 결과 안내는 끝낸다.
    if (!messenger.mounted) {
      return;
    }
    container.invalidate(boardProvider);
    _announceDeletion(messenger, outcome);
  }

  void _announceDeletion(
    ScaffoldMessengerState messenger,
    TaskDeletionOutcome outcome,
  ) {
    final message = switch (outcome) {
      TaskDeletionSucceeded(:final failedPeerCount) when failedPeerCount > 0 =>
        '다른 데스크탑 $failedPeerCount곳에는 아직 남아 있습니다. 그 데스크탑이 켜지면 직접 삭제해 주세요.',
      TaskDeletionFailed(:final message) => message,
      TaskDeletionSucceeded() || TaskDeletionNeedsForce() => null,
      TaskDeletionNeedsRegistrationRemoval() => 'Git 저장소를 찾을 수 없습니다.',
    };
    if (message != null) {
      messenger.showSnackBar(SnackBar(content: Text(message)));
    }
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final unreadCount = ref.watch(
      notificationInboxProvider.select(
        (value) => value.value?.unreadCountForTask(task.id) ?? 0,
      ),
    );
    return Material(
      color: context.colors.bgSurface,
      borderRadius: BorderRadius.circular(AppSizes.radiusLg),
      child: InkWell(
        borderRadius: BorderRadius.circular(AppSizes.radiusLg),
        onTap: onOpen,
        onLongPress: () => _deleteTask(context, ref),
        child: Container(
          constraints: const BoxConstraints(minHeight: AppSizes.minTouchTarget),
          padding: const EdgeInsets.all(AppSizes.p12),
          decoration: BoxDecoration(
            border: Border.all(
              width: isFindMatch || isSelected ? 2 : 1,
              color: isFindMatch || isSelected
                  ? AppColors.brandPrimary
                  : task.projectName == null
                  ? context.colors.borderDefault
                  : projectColor(task.projectName!, task.projectColor),
            ),
            borderRadius: BorderRadius.circular(AppSizes.radiusLg),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  if (task.projectIconDataUrl != null)
                    Padding(
                      padding: const EdgeInsets.only(right: AppSizes.p8),
                      child: TaskProjectIcon(dataUrl: task.projectIconDataUrl),
                    ),
                  Expanded(
                    child: Text(
                      task.title,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      style: Theme.of(context).textTheme.bodyMedium,
                    ),
                  ),
                  if (unreadCount > 0) ...[
                    const SizedBox(width: AppSizes.p12),
                    Semantics(
                      key: ValueKey('task-unread-${task.id}'),
                      identifier: 'task-unread-${task.id}',
                      label: '읽지 않은 알림 $unreadCount개',
                      excludeSemantics: true,
                      child: Badge.count(
                        count: unreadCount,
                        child: const Icon(
                          Icons.notifications_outlined,
                          size: 20,
                          color: AppColors.statusProgress,
                        ),
                      ),
                    ),
                  ],
                ],
              ),
              if (task.description?.isNotEmpty ?? false)
                Padding(
                  padding: const EdgeInsets.only(top: AppSizes.p4),
                  child: Text(
                    task.description!,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                ),
              if (task.branchName != null)
                TaskDiffSummary(taskId: task.id, compact: true),
              RunningAiBadge(
                worktreePath: task.worktreePath,
                onPressed: () => Navigator.of(context).push(
                  MaterialPageRoute<void>(
                    builder: (_) => LiveAiSessionsScreen(
                      taskId: task.id,
                      onSelectWindow: (windowId) {
                        Navigator.of(context).pushReplacement(
                          MaterialPageRoute<void>(
                            builder: (_) => TaskDetailScreen(
                              task: task,
                              initialWindowId: windowId,
                            ),
                          ),
                        );
                      },
                    ),
                  ),
                ),
              ),
              const SizedBox(height: AppSizes.p8),
              Wrap(
                spacing: AppSizes.p4,
                runSpacing: AppSizes.p4,
                children: [
                  if (displayPriority != null)
                    _TaskBadge(
                      label:
                          '${switch (displayPriority!) {
                            TaskPriority.high => 'P1',
                            TaskPriority.medium => 'P2',
                            TaskPriority.low => 'P3',
                          }}${isPriorityInherited ? ' ↓' : ''}',
                      color: AppColors.brandPrimary,
                    ),
                  if (isBaseProject)
                    const _TaskBadge(
                      label: 'Base',
                      color: AppColors.brandPrimary,
                    ),
                  if (task.agentType != null)
                    _TaskBadge(
                      label: task.agentType!,
                      color: context.colors.textSecondary,
                    ),
                  if (task.sessionType != null)
                    _TaskBadge(
                      label: task.sessionType!,
                      color: context.colors.textSecondary,
                    ),
                  if (task.projectName != null)
                    _TaskBadge(
                      label: task.projectName!,
                      color: projectColor(task.projectName!, task.projectColor),
                      background: projectColor(
                        task.projectName!,
                        task.projectColor,
                      ).withValues(alpha: 0.16),
                    ),
                  if (task.branchName != null)
                    _TaskBadge(
                      label: task.branchName!,
                      isMono: true,
                      color: const Color(0xFFAEB3BD),
                      background: const Color(0x0FFFFFFF),
                    ),
                  if (task.isRemote)
                    _TaskBadge(
                      label: task.sshHost!,
                      isMono: true,
                      color: const Color(0xFF9BE6B4),
                      background: const Color(0x2965D08A),
                    ),
                  if (task.prUrl != null)
                    Semantics(
                      identifier: 'task-pr-${task.id}',
                      child: TextButton(
                        onPressed: () => _openPr(context),
                        style: TextButton.styleFrom(
                          minimumSize: const Size(48, 48),
                          padding: const EdgeInsets.symmetric(
                            horizontal: AppSizes.p8,
                          ),
                        ),
                        child: const Text('PR'),
                      ),
                    ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _TaskBadge extends StatelessWidget {
  const _TaskBadge({
    required this.label,
    this.isMono = false,
    this.color,
    this.background,
  });

  final String label;
  final bool isMono;
  final Color? color;
  final Color? background;

  @override
  Widget build(BuildContext context) {
    final foreground = Theme.of(context).brightness == Brightness.light
        ? context.colors.textPrimary
        : color ?? context.colors.textSecondary;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: AppSizes.p8, vertical: 2),
      decoration: BoxDecoration(
        color:
            background ??
            (color == null
                ? context.colors.buttonNeutral
                : AppColors.brandSubtle),
        borderRadius: BorderRadius.circular(AppSizes.radiusSm),
      ),
      child: Text(
        label,
        overflow: TextOverflow.ellipsis,
        style: isMono
            ? monoStyle(fontSize: 11, color: foreground)
            : Theme.of(context).textTheme.labelSmall
                  ?.copyWith(color: foreground),
      ),
    );
  }
}

class _EmptyStatus extends StatelessWidget {
  const _EmptyStatus();

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: AppSizes.p16),
      child: Center(
        child: Text(
          context.tr('이 상태에는 태스크가 없습니다.'),
          style: Theme.of(context).textTheme.bodySmall,
        ),
      ),
    );
  }
}

class _BoardLoadFailure extends StatelessWidget {
  const _BoardLoadFailure({required this.onRetry});

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
              context.tr(
                '데스크탑에 연결하지 못했습니다.\n같은 네트워크에 있는지, KanVibe가 켜져 있는지 확인해 주세요.',
              ),
              textAlign: TextAlign.center,
              style: Theme.of(context).textTheme.bodyMedium,
            ),
            const SizedBox(height: AppSizes.p16),
            FilledButton(onPressed: onRetry, child: Text(context.tr('다시 시도'))),
          ],
        ),
      ),
    );
  }
}
