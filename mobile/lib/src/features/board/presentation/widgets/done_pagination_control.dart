import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/l10n/app_localizations.dart';
import '../../application/board_pagination_controller.dart';
import '../../domain/task_status.dart';

class DonePaginationControl extends ConsumerWidget {
  const DonePaginationControl({super.key});
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final state = ref.watch(boardPaginationProvider).value;
    if (state == null) return const SizedBox.shrink();
    return Column(
      children: [
        Text(
          context.tr('완료 {loaded} / 전체 {total}', {
            'loaded': state.snapshot.tasksIn(TaskStatus.done).length,
            'total': state.snapshot.doneTotal,
          }),
          style: Theme.of(context).textTheme.bodySmall,
        ),
        if (state.snapshot.hasMoreDone)
          Semantics(
            identifier: 'done-load-more',
            child: TextButton(
              onPressed: state.isLoadingMore
                  ? null
                  : () => ref.read(boardPaginationProvider.notifier).loadMore(),
              child: Text(
                context.tr(
                  state.isLoadingMore
                      ? '불러오는 중…'
                      : state.loadMoreFailed
                      ? '완료 태스크 다시 불러오기'
                      : '완료 태스크 더 보기',
                ),
              ),
            ),
          ),
      ],
    );
  }
}
