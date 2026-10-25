import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../../../common/constants/app_theme.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../domain/saved_connections.dart';
import 'connection_screen.dart';

/// 페어링한 PC를 보관하고 현재 작업할 PC를 고르는 화면.
class DesktopConnectionsScreen extends ConsumerStatefulWidget {
  const DesktopConnectionsScreen({super.key});

  @override
  ConsumerState<DesktopConnectionsScreen> createState() =>
      _DesktopConnectionsScreenState();
}

class _DesktopConnectionsScreenState
    extends ConsumerState<DesktopConnectionsScreen> {
  String? _busyId;

  Future<void> _select(DesktopConnection connection) async {
    if (_busyId != null) return;
    setState(() => _busyId = connection.id);
    try {
      await ref
          .read(connectionControllerProvider.notifier)
          .select(connection.id);
      if (mounted && Navigator.of(context).canPop()) {
        Navigator.of(context).popUntil((route) => route.isFirst);
      }
    } catch (_) {
      if (mounted) _showMessage('PC를 선택하지 못했습니다. 다시 시도해 주세요.');
    } finally {
      if (mounted) setState(() => _busyId = null);
    }
  }

  Future<void> _remove(DesktopConnection connection) async {
    if (_busyId != null) return;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text('${connection.displayName} 삭제'),
        content: const Text('이 PC의 연결 정보를 삭제합니다. 다시 연결하려면 PC에서 새 코드를 받아야 합니다.'),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: const Text('취소'),
          ),
          TextButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: const Text('삭제'),
          ),
        ],
      ),
    );
    if (confirmed != true || !mounted) return;

    final wasActive =
        ref.read(connectionControllerProvider).value?.activeId == connection.id;
    setState(() => _busyId = connection.id);
    try {
      final toldDesktop = await ref
          .read(connectionControllerProvider.notifier)
          .remove(connection.id);
      if (!mounted) return;
      if (wasActive && Navigator.of(context).canPop()) {
        Navigator.of(context).popUntil((route) => route.isFirst);
      }
      if (!toldDesktop) {
        _showMessage('PC에 알리지 못했습니다. PC의 KanVibe 설정에서도 이 기기를 삭제해 주세요.');
      }
    } catch (_) {
      if (mounted) _showMessage('PC 연결 정보를 삭제하지 못했습니다. 다시 시도해 주세요.');
    } finally {
      if (mounted) setState(() => _busyId = null);
    }
  }

  void _showMessage(String message) {
    ScaffoldMessenger.of(context)
        .showSnackBar(SnackBar(content: Text(message)));
  }

  void _add() {
    if (_busyId != null) return;
    Navigator.of(context).push(
      MaterialPageRoute<void>(
        builder: (_) => const ConnectionScreen(isAdding: true),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final registry =
        ref.watch(connectionControllerProvider).value ??
        const SavedConnections();

    return Scaffold(
      appBar: AppBar(title: const Text('연결된 PC')),
      body: SafeArea(
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 640),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Padding(
                  padding: const EdgeInsets.fromLTRB(
                    AppSizes.p16,
                    AppSizes.p16,
                    AppSizes.p16,
                    AppSizes.p12,
                  ),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        '${registry.connections.length}대의 PC',
                        style: Theme.of(context).textTheme.titleMedium,
                      ),
                      const SizedBox(height: AppSizes.p4),
                      Text(
                        registry.activeId == null
                            ? '작업할 PC를 선택해 주세요.'
                            : 'PC를 선택하면 해당 PC의 보드로 이동합니다.',
                        style: Theme.of(context).textTheme.bodySmall,
                      ),
                    ],
                  ),
                ),
                Expanded(
                  child: ListView.separated(
                    itemCount: registry.connections.length,
                    separatorBuilder: (_, _) => const Divider(height: 1),
                    itemBuilder: (context, index) {
                      final connection = registry.connections[index];
                      final selected = connection.id == registry.activeId;
                      return ListTile(
                        key: ValueKey('pc-${connection.id}'),
                        contentPadding: const EdgeInsets.symmetric(
                          horizontal: AppSizes.p16,
                          vertical: AppSizes.p4,
                        ),
                        minTileHeight: AppSizes.minTouchTarget,
                        leading: Icon(
                          Icons.computer,
                          color: selected
                              ? AppColors.brandPrimary
                              : AppColors.textSecondary,
                        ),
                        title: Text(
                          connection.displayName,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                        ),
                        subtitle: Text(
                          connection.address,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: monoStyle(fontSize: 12),
                        ),
                        selected: selected,
                        selectedTileColor: AppColors.brandSubtle,
                        trailing: Row(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            if (selected)
                              const Icon(
                                Icons.check_circle,
                                color: AppColors.brandPrimary,
                                semanticLabel: '현재 PC',
                              ),
                            IconButton(
                              onPressed: _busyId == null
                                  ? () => _remove(connection)
                                  : null,
                              tooltip: '${connection.displayName} 삭제',
                              icon: const Icon(Icons.delete_outline),
                            ),
                          ],
                        ),
                        onTap: _busyId == null
                            ? () => _select(connection)
                            : null,
                      );
                    },
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.all(AppSizes.p16),
                  child: FilledButton.icon(
                    onPressed: _busyId == null ? _add : null,
                    icon: const Icon(Icons.add),
                    label: const Text('PC 추가'),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
