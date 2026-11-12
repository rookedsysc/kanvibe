import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_sizes.dart';
import '../../../../common/providers.dart';
import '../../application/board_desktop_filter_controller.dart';

class DesktopFilterButton extends ConsumerWidget {
  const DesktopFilterButton({super.key});

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
        : ref.watch(boardDesktopFilterProvider(connectionId));
    final available = {
      'local',
      for (final host in board.value?.desktopHosts ?? const <String>[])
        'ssh:$host',
    };
    final count = selected.intersection(available).length;

    return Semantics(
      key: const ValueKey('desktop-filter'),
      identifier: 'desktop-filter',
      selected: count > 0,
      child: OutlinedButton(
        onPressed: connectionId == null || !board.hasValue || board.isLoading
            ? null
            : () => showModalBottomSheet<void>(
                context: context,
                useSafeArea: true,
                constraints: const BoxConstraints(
                  maxWidth: AppSizes.tabletBreakpoint,
                ),
                builder: (_) => const _DesktopFilterSheet(),
              ),
        style: OutlinedButton.styleFrom(
          foregroundColor: AppColors.textPrimary,
          backgroundColor: AppColors.bgPage,
          side: const BorderSide(color: AppColors.borderDefault),
          minimumSize: const Size(
            AppSizes.minTouchTarget,
            AppSizes.minTouchTarget,
          ),
          padding: const EdgeInsets.symmetric(horizontal: AppSizes.p8),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(AppSizes.radiusMd),
          ),
        ),
        child: Row(
          children: [
            const Icon(Icons.desktop_windows_outlined, size: 18),
            const SizedBox(width: AppSizes.p8),
            Flexible(
              child: Text(
                count == 0 ? '전체 데스크톱' : '데스크톱 $count개',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
              ),
            ),
            const Icon(Icons.keyboard_arrow_down, size: 18),
          ],
        ),
      ),
    );
  }
}

class _DesktopFilterSheet extends ConsumerWidget {
  const _DesktopFilterSheet();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final connectionId = ref.watch(
      connectionControllerProvider.select(
        (value) => value.value?.activeConnection?.id,
      ),
    );
    final hosts =
        ref.watch(boardProvider).value?.desktopHosts ?? const <String>[];
    final selected = connectionId == null
        ? const <String>{}
        : ref.watch(boardDesktopFilterProvider(connectionId));
    final options = {
      '': '전체 데스크톱',
      'local': '로컬',
      for (final host in hosts) 'ssh:$host': host,
    };
    final active = selected.intersection(options.keys.toSet());

    return SafeArea(
      child: ConstrainedBox(
        constraints: BoxConstraints(
          maxHeight: MediaQuery.sizeOf(context).height * 0.7,
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
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
                      '데스크톱',
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                  ),
                  Semantics(
                    key: const ValueKey('desktop-filter-close'),
                    identifier: 'desktop-filter-close',
                    child: IconButton(
                      onPressed: () => Navigator.of(context).pop(),
                      tooltip: '닫기',
                      icon: const Icon(Icons.close),
                    ),
                  ),
                ],
              ),
            ),
            Flexible(
              child: ListView(
                shrinkWrap: true,
                children: [
                  for (final entry in options.entries)
                    Semantics(
                      key: ValueKey(
                        'desktop-filter-${entry.key.isEmpty ? 'all' : entry.key.replaceFirst('ssh:', 'ssh-')}',
                      ),
                      identifier:
                          'desktop-filter-${entry.key.isEmpty ? 'all' : entry.key.replaceFirst('ssh:', 'ssh-')}',
                      child: CheckboxListTile(
                        value: entry.key.isEmpty
                            ? active.isEmpty
                            : active.contains(entry.key),
                        onChanged: connectionId == null
                            ? null
                            : (_) {
                                final controller = ref.read(
                                  boardDesktopFilterProvider(connectionId)
                                      .notifier,
                                );
                                if (entry.key.isEmpty) {
                                  controller.showAllDesktops();
                                } else {
                                  controller.toggleDesktop(entry.key);
                                }
                              },
                        title: Text(entry.value),
                        controlAffinity: ListTileControlAffinity.leading,
                        contentPadding: const EdgeInsets.symmetric(
                          horizontal: AppSizes.p16,
                        ),
                        checkColor: AppColors.textPrimary,
                        fillColor: const WidgetStatePropertyAll(
                          Colors.transparent,
                        ),
                        side: WidgetStateBorderSide.resolveWith(
                          (_) =>
                              const BorderSide(color: AppColors.textSecondary),
                        ),
                      ),
                    ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}
