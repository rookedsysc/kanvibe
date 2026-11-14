import '../../../../common/l10n/app_localizations.dart';

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_sizes.dart';
import '../../../../common/constants/project_colors.dart';
import '../../../../common/providers.dart';
import '../../application/board_project_filter_controller.dart';

class ProjectFilterButton extends ConsumerWidget {
  const ProjectFilterButton({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final board = ref.watch(boardProvider);
    final connectionId = ref.watch(
      connectionControllerProvider.select(
        (value) => value.value?.activeConnection?.id,
      ),
    );
    final selected = connectionId == null
        ? const <String>{}
        : ref.watch(boardProjectFilterProvider(connectionId));
    final count =
        board.value?.creatableProjects
            .where((project) => selected.contains(project.id))
            .length ??
        0;

    return Semantics(
      key: const ValueKey('project-filter'),
      identifier: 'project-filter',
      selected: count > 0,
      value: count == 0
          ? context.tr('전체 프로젝트')
          : context.tr('{count}개 선택', {'count': count}),
      child: IconButton(
        onPressed: connectionId == null || !board.hasValue || board.isLoading
            ? null
            : () => showModalBottomSheet<void>(
                context: context,
                isScrollControlled: true,
                useSafeArea: true,
                constraints: const BoxConstraints(
                  maxWidth: AppSizes.tabletBreakpoint,
                ),
                builder: (_) => const _ProjectFilterSheet(),
              ),
        constraints: const BoxConstraints(
          minWidth: AppSizes.minTouchTarget,
          minHeight: AppSizes.minTouchTarget,
        ),
        tooltip: context.tr('프로젝트 필터'),
        color: count > 0 ? AppColors.brandPrimary : null,
        icon: Badge(
          isLabelVisible: count > 0,
          backgroundColor: AppColors.brandPrimary,
          textColor: Theme.of(context).colorScheme.onPrimary,
          label: Text('$count'),
          child: const Icon(Icons.filter_list),
        ),
      ),
    );
  }
}

class _ProjectFilterSheet extends ConsumerStatefulWidget {
  const _ProjectFilterSheet();

  @override
  ConsumerState<_ProjectFilterSheet> createState() =>
      _ProjectFilterSheetState();
}

class _ProjectFilterSheetState extends ConsumerState<_ProjectFilterSheet> {
  String _query = '';
  Timer? _searchDebounce;
  static const _searchDelay = Duration(milliseconds: 300);

  void _search(String value) {
    _searchDebounce?.cancel();
    _searchDebounce = Timer(_searchDelay, () {
      setState(() => _query = value);
    });
  }

  @override
  void dispose() {
    _searchDebounce?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final board = ref.watch(boardProvider);
    final connectionId = ref.watch(
      connectionControllerProvider.select(
        (value) => value.value?.activeConnection?.id,
      ),
    );
    final selected = connectionId == null
        ? const <String>{}
        : ref.watch(boardProjectFilterProvider(connectionId));
    final projects = board.value?.creatableProjects ?? const [];
    final hasSelection = projects.any(
      (project) => selected.contains(project.id),
    );
    final matches = projects
        .where((project) => project.matchesSearch(_query))
        .toList();

    return Padding(
      padding: EdgeInsets.only(bottom: MediaQuery.viewInsetsOf(context).bottom),
      child: FractionallySizedBox(
        // 검색·복수 선택을 함께 보이면서 뒤의 보드 문맥도 남긴다.
        heightFactor: 0.85,
        child: SafeArea(
          top: false,
          child: Column(
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(
                  AppSizes.p16,
                  AppSizes.p8,
                  AppSizes.p8,
                  AppSizes.p8,
                ),
                child: Row(
                  children: [
                    Expanded(
                      child: Text(
                        context.tr('프로젝트 필터'),
                        style: Theme.of(context).textTheme.titleMedium,
                      ),
                    ),
                    Semantics(
                      key: const ValueKey('project-filter-close'),
                      identifier: 'project-filter-close',
                      child: IconButton(
                        onPressed: () => Navigator.of(context).pop(),
                        tooltip: context.tr('닫기'),
                        constraints: const BoxConstraints(
                          minWidth: AppSizes.minTouchTarget,
                          minHeight: AppSizes.minTouchTarget,
                        ),
                        icon: const Icon(Icons.close),
                      ),
                    ),
                  ],
                ),
              ),
              Expanded(
                child: CustomScrollView(
                  slivers: [
                    SliverPadding(
                      padding: const EdgeInsets.symmetric(
                        horizontal: AppSizes.p16,
                      ),
                      sliver: SliverToBoxAdapter(
                        child: Semantics(
                          identifier: 'project-filter-search',
                          child: TextField(
                            onChanged: _search,
                            decoration: InputDecoration(
                              labelText: context.tr('프로젝트 검색'),
                              prefixIcon: Icon(Icons.search),
                            ),
                          ),
                        ),
                      ),
                    ),
                    SliverToBoxAdapter(
                      child: Semantics(
                        key: const ValueKey('project-filter-all'),
                        identifier: 'project-filter-all',
                        child: CheckboxListTile(
                          value: !hasSelection,
                          onChanged: connectionId == null
                              ? null
                              : (_) => ref
                                    .read(
                                      boardProjectFilterProvider(connectionId)
                                          .notifier,
                                    )
                                    .showAllProjects(),
                          title: Text(context.tr('전체 프로젝트')),
                          contentPadding: const EdgeInsets.symmetric(
                            horizontal: AppSizes.p16,
                            vertical: AppSizes.p4,
                          ),
                        ),
                      ),
                    ),
                    if (board.isLoading && !board.hasValue)
                      const SliverToBoxAdapter(
                        child: Center(child: CircularProgressIndicator()),
                      )
                    else if (board.hasError)
                      SliverToBoxAdapter(
                        child: Center(
                          child: TextButton(
                            onPressed: () => ref.invalidate(boardProvider),
                            child: Text(context.tr('프로젝트를 불러오지 못했습니다. 다시 시도')),
                          ),
                        ),
                      )
                    else if (matches.isEmpty)
                      SliverToBoxAdapter(
                        child: Padding(
                          padding: const EdgeInsets.all(AppSizes.p24),
                          child: Text(
                            context.tr(
                              projects.isEmpty
                                  ? '등록된 프로젝트가 없습니다.'
                                  : '검색 결과가 없습니다.',
                            ),
                            textAlign: TextAlign.center,
                          ),
                        ),
                      )
                    else
                      SliverList.builder(
                        itemCount: matches.length,
                        itemBuilder: (context, index) {
                          final project = matches[index];
                          return Semantics(
                            key: ValueKey(
                              'project-filter-option-${project.id}',
                            ),
                            identifier: 'project-filter-option-${project.id}',
                            child: CheckboxListTile(
                              value: selected.contains(project.id),
                              onChanged: connectionId == null
                                  ? null
                                  : (_) => ref
                                        .read(
                                          boardProjectFilterProvider(
                                            connectionId,
                                          ).notifier,
                                        )
                                        .toggleProject(project.id),
                              secondary: Container(
                                width: AppSizes.p12,
                                height: AppSizes.p12,
                                decoration: BoxDecoration(
                                  color: projectColor(
                                    project.name,
                                    project.color,
                                  ),
                                  shape: BoxShape.circle,
                                ),
                              ),
                              title: Text(
                                project.name,
                                maxLines: 2,
                                overflow: TextOverflow.ellipsis,
                              ),
                              subtitle: Text(
                                [
                                  project.sshHost ?? context.tr('로컬'),
                                  if (project.repoPath != null)
                                    project.repoPath!,
                                ].join(' · '),
                                maxLines: 2,
                                overflow: TextOverflow.ellipsis,
                              ),
                              contentPadding: const EdgeInsets.symmetric(
                                horizontal: AppSizes.p16,
                                vertical: AppSizes.p4,
                              ),
                            ),
                          );
                        },
                      ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
