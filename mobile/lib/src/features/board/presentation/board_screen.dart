import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../../../common/constants/app_theme.dart';
import '../../../common/providers.dart';
import '../../task_detail/presentation/task_detail_screen.dart';
import '../data/board_snapshot.dart';
import '../domain/board_task.dart';
import '../domain/task_status.dart';

/// 첫 화면. 태스크를 상태별로 나눠 보여 준다.
///
/// 폰과 태블릿은 같은 데이터를 다르게 놓는다. 좁은 화면에서 칸반을 옆으로 밀어 보게 하면
/// 한 번에 한 칸밖에 못 보므로, 폰은 상태를 세로 구획으로 쌓고 태블릿만 칸반으로 편다.
/// 갈림길은 [isTabletLayout] 하나뿐이고 데이터는 한 번만 불러온다.
class BoardScreen extends ConsumerWidget {
  const BoardScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final board = ref.watch(boardProvider);

    return Scaffold(
      appBar: AppBar(
        title: const Text('KanVibe'),
        actions: [
          IconButton(
            onPressed: () => ref.invalidate(boardProvider),
            icon: const Icon(Icons.refresh),
            tooltip: '새로고침',
          ),
          IconButton(
            onPressed: () =>
                ref.read(connectionControllerProvider.notifier).disconnect(),
            icon: const Icon(Icons.link_off),
            tooltip: '연결 끊기',
          ),
        ],
      ),
      body: SafeArea(
        child: board.when(
          loading: () => const Center(child: CircularProgressIndicator()),
          error: (error, _) =>
              _BoardLoadFailure(onRetry: () => ref.invalidate(boardProvider)),
          data: (snapshot) => RefreshIndicator(
            onRefresh: () async => ref.invalidate(boardProvider),
            child: LayoutBuilder(
              builder: (context, constraints) => isTabletLayout(constraints)
                  ? _KanbanBoard(snapshot: snapshot)
                  : _StatusListView(snapshot: snapshot),
            ),
          ),
        ),
      ),
    );
  }
}

/// 태블릿 배치. 상태별 칸을 옆으로 늘어놓는다
class _KanbanBoard extends StatelessWidget {
  const _KanbanBoard({required this.snapshot});

  final BoardSnapshot snapshot;

  /// 칸 하나의 너비. 좁으면 제목이 잘리고 넓으면 한 화면에 세 칸도 안 들어간다
  static const _columnWidth = 320.0;

  @override
  Widget build(BuildContext context) {
    return ListView.separated(
      scrollDirection: Axis.horizontal,
      padding: const EdgeInsets.all(AppSizes.p16),
      itemCount: TaskStatus.values.length,
      separatorBuilder: (_, _) => const SizedBox(width: AppSizes.p12),
      itemBuilder: (context, index) {
        final status = TaskStatus.values[index];
        final tasks = snapshot.tasksIn(status);

        return SizedBox(
          width: _columnWidth,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              _StatusHeading(status: status, count: tasks.length),
              const SizedBox(height: AppSizes.p8),
              Expanded(
                child: tasks.isEmpty
                    ? const _EmptyStatus()
                    : ListView.separated(
                        itemCount: tasks.length,
                        separatorBuilder: (_, _) =>
                            const SizedBox(height: AppSizes.p8),
                        itemBuilder: (context, taskIndex) => _TaskCard(
                          task: tasks[taskIndex],
                          key: ValueKey(tasks[taskIndex].id),
                        ),
                      ),
              ),
            ],
          ),
        );
      },
    );
  }
}

/// 폰 배치. 상태를 세로로 쌓고 머리글을 화면 위에 붙여 둔다
class _StatusListView extends StatelessWidget {
  const _StatusListView({required this.snapshot});

  final BoardSnapshot snapshot;

  @override
  Widget build(BuildContext context) {
    if (snapshot.isEmpty) {
      return ListView(
        children: const [
          SizedBox(height: AppSizes.p32),
          _EmptyStatus(message: '아직 태스크가 없습니다.'),
        ],
      );
    }

    return CustomScrollView(
      slivers: [
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
            sliver: snapshot.tasksIn(status).isEmpty
                ? const SliverToBoxAdapter(child: _EmptyStatus())
                : SliverList.separated(
                    itemCount: snapshot.tasksIn(status).length,
                    separatorBuilder: (_, _) =>
                        const SizedBox(height: AppSizes.p8),
                    itemBuilder: (context, index) {
                      final task = snapshot.tasksIn(status)[index];
                      return _TaskCard(task: task, key: ValueKey(task.id));
                    },
                  ),
          ),
        ],
      ],
    );
  }
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
      color: AppColors.bgPage,
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

class _TaskCard extends StatelessWidget {
  const _TaskCard({required this.task, super.key});

  final BoardTask task;

  @override
  Widget build(BuildContext context) {
    return Material(
      color: AppColors.bgSurface,
      borderRadius: BorderRadius.circular(AppSizes.radiusLg),
      child: InkWell(
        borderRadius: BorderRadius.circular(AppSizes.radiusLg),
        onTap: () => Navigator.of(context).push(
          MaterialPageRoute<void>(
            builder: (_) =>
                TaskDetailScreen(taskId: task.id, taskTitle: task.title),
          ),
        ),
        child: Container(
          constraints: const BoxConstraints(minHeight: AppSizes.minTouchTarget),
          padding: const EdgeInsets.all(AppSizes.p12),
          decoration: BoxDecoration(
            border: Border.all(color: AppColors.borderDefault),
            borderRadius: BorderRadius.circular(AppSizes.radiusLg),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                task.title,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.bodyMedium,
              ),
              const SizedBox(height: AppSizes.p8),
              Wrap(
                spacing: AppSizes.p4,
                runSpacing: AppSizes.p4,
                children: [
                  if (task.projectName != null)
                    _TaskBadge(label: task.projectName!),
                  if (task.branchName != null)
                    _TaskBadge(label: task.branchName!, isMono: true),
                  if (task.isRemote)
                    _TaskBadge(label: task.sshHost!, isMono: true),
                  if (task.prUrl != null)
                    const _TaskBadge(
                      label: 'PR',
                      color: AppColors.brandPrimary,
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
  const _TaskBadge({required this.label, this.isMono = false, this.color});

  final String label;
  final bool isMono;
  final Color? color;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: AppSizes.p8, vertical: 2),
      decoration: BoxDecoration(
        color: color == null ? AppColors.buttonNeutral : AppColors.brandSubtle,
        borderRadius: BorderRadius.circular(AppSizes.radiusSm),
      ),
      child: Text(
        label,
        overflow: TextOverflow.ellipsis,
        style: isMono
            ? monoStyle(fontSize: 11, color: color ?? AppColors.textSecondary)
            : Theme.of(context).textTheme.labelSmall
                  ?.copyWith(color: color ?? AppColors.textSecondary),
      ),
    );
  }
}

class _EmptyStatus extends StatelessWidget {
  const _EmptyStatus({this.message = '이 상태에는 태스크가 없습니다.'});

  final String message;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: AppSizes.p16),
      child: Center(
        child: Text(message, style: Theme.of(context).textTheme.bodySmall),
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
              '데스크탑에 연결하지 못했습니다.\n같은 네트워크에 있는지, KanVibe가 켜져 있는지 확인해 주세요.',
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
