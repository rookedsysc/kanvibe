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
        ? ''
        : ref.watch(boardDesktopFilterProvider(connectionId));
    final hosts = board.value?.desktopHosts ?? const <String>[];
    final active =
        selected == 'local' || hosts.any((host) => selected == 'ssh:$host')
        ? selected
        : '';
    final label = active == ''
        ? '전체 데스크톱'
        : active == 'local'
        ? '로컬'
        : active.substring('ssh:'.length);

    return Semantics(
      key: const ValueKey('desktop-filter'),
      identifier: 'desktop-filter',
      selected: active != '',
      child: PopupMenuButton<String>(
        tooltip: '데스크톱 필터',
        enabled: connectionId != null && board.hasValue && !board.isLoading,
        initialValue: active,
        onSelected: (desktop) => ref
            .read(boardDesktopFilterProvider(connectionId!).notifier)
            .select(desktop),
        itemBuilder: (_) => [
          for (final entry in {
            '': '전체 데스크톱',
            'local': '로컬',
            for (final host in hosts) 'ssh:$host': host,
          }.entries)
            CheckedPopupMenuItem(
              key: ValueKey(
                'desktop-filter-${entry.key.isEmpty ? 'all' : entry.key.replaceFirst('ssh:', 'ssh-')}',
              ),
              value: entry.key,
              checked: active == entry.key,
              child: Semantics(
                identifier:
                    'desktop-filter-${entry.key.isEmpty ? 'all' : entry.key.replaceFirst('ssh:', 'ssh-')}',
                child: Text(entry.value),
              ),
            ),
        ],
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: AppSizes.p8),
          child: ConstrainedBox(
            constraints: const BoxConstraints(
              minHeight: AppSizes.minTouchTarget,
            ),
            child: Row(
              children: [
                Flexible(
                  child: Text(
                    label,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      color: active.isEmpty
                          ? AppColors.textPrimary
                          : AppColors.brandPrimary,
                    ),
                  ),
                ),
                const Icon(Icons.arrow_drop_down),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
