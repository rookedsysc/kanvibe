import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/l10n/app_localizations.dart';
import '../../../../common/providers.dart';
import '../../application/terminal_management_controller.dart';

class TerminalManagementSheet extends ConsumerWidget {
  const TerminalManagementSheet({
    required this.taskId,
    required this.selectedPaneId,
    required this.isRemote,
    super.key,
  });
  final String taskId;
  final String selectedPaneId;
  final bool isRemote;

  Future<void> _rename(
    BuildContext context,
    WidgetRef ref,
    String tabId,
    String currentName,
  ) async {
    final connection = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    final identity = connection == null
        ? null
        : (connection.id, connection.token);
    var draftName = currentName;
    final name = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(context.tr('탭 이름 변경')),
        content: TextFormField(
          key: const ValueKey('terminal-tab-name'),
          initialValue: currentName,
          onChanged: (value) => draftName = value,
          autofocus: true,
          maxLength: 128,
          decoration: InputDecoration(labelText: context.tr('탭 이름')),
          onFieldSubmitted: (value) => Navigator.pop(context, value),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: Text(context.tr('취소')),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, draftName),
            child: Text(context.tr('변경')),
          ),
        ],
      ),
    );
    if (!context.mounted) return;
    final current = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    final currentIdentity = current == null
        ? null
        : (current.id, current.token);
    if (name != null && identity != null && identity == currentIdentity) {
      await ref
          .read(terminalManagementProvider(taskId).notifier)
          .rename(tabId, name);
    }
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final operations = ref.watch(terminalManagementProvider(taskId));
    final surfaces = ref.watch(surfacesProvider(taskId));
    return SafeArea(
      child: FractionallySizedBox(
        heightFactor: 0.85,
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.all(16),
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      context.tr('터미널 관리'),
                      style: Theme.of(context).textTheme.titleLarge,
                    ),
                  ),
                  IconButton(
                    tooltip: context.tr('닫기'),
                    onPressed: () => Navigator.pop(context),
                    icon: const Icon(Icons.close),
                  ),
                ],
              ),
            ),
            if (isRemote)
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 16),
                child: FilledButton.icon(
                  key: const ValueKey('paste-clipboard-image'),
                  onPressed: operations.isBusy
                      ? null
                      : () => ref
                            .read(terminalManagementProvider(taskId).notifier)
                            .pasteImage(selectedPaneId),
                  icon: const Icon(Icons.image_outlined),
                  label: Text(context.tr('클립보드 이미지 붙여넣기')),
                ),
              ),
            if (operations.isBusy) const LinearProgressIndicator(),
            if (operations.message != null)
              Padding(
                padding: const EdgeInsets.all(16),
                child: Text(
                  context.tr(operations.message!),
                  semanticsLabel: context.tr(operations.message!),
                ),
              ),
            Expanded(
              child: surfaces.when(
                loading: () => const Center(child: CircularProgressIndicator()),
                error: (_, _) => Center(
                  child: FilledButton(
                    onPressed: () => ref.invalidate(surfacesProvider(taskId)),
                    child: Text(context.tr('탭 다시 조회')),
                  ),
                ),
                data: (value) {
                  final tabs = value.surfaces.tabs;
                  return ReorderableListView.builder(
                    buildDefaultDragHandles: false,
                    itemCount: tabs.length,
                    onReorderItem: (oldIndex, newIndex) {
                      if (!operations.isBusy) {
                        ref
                            .read(terminalManagementProvider(taskId).notifier)
                            .move(tabs[oldIndex].id, newIndex);
                      }
                    },
                    itemBuilder: (context, index) => ListTile(
                      key: ValueKey('manage-tab-${tabs[index].id}'),
                      title: Text(tabs[index].name),
                      onTap: operations.isBusy
                          ? null
                          : () => _rename(
                              context,
                              ref,
                              tabs[index].id,
                              tabs[index].name,
                            ),
                      subtitle: Text(context.tr('탭 이름 변경')),
                      trailing: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          IconButton(
                            tooltip: context.tr('탭 앞으로 이동'),
                            onPressed: operations.isBusy || index == 0
                                ? null
                                : () => ref
                                      .read(
                                        terminalManagementProvider(taskId)
                                            .notifier,
                                      )
                                      .move(tabs[index].id, index - 1),
                            icon: const Icon(Icons.arrow_upward),
                          ),
                          IconButton(
                            tooltip: context.tr('탭 뒤로 이동'),
                            onPressed:
                                operations.isBusy || index == tabs.length - 1
                                ? null
                                : () => ref
                                      .read(
                                        terminalManagementProvider(taskId)
                                            .notifier,
                                      )
                                      .move(tabs[index].id, index + 1),
                            icon: const Icon(Icons.arrow_downward),
                          ),
                          ReorderableDragStartListener(
                            index: index,
                            enabled: !operations.isBusy,
                            child: Semantics(
                              label: context.tr('탭 순서 끌어서 변경'),
                              child: const SizedBox(
                                width: 48,
                                height: 48,
                                child: Icon(Icons.drag_handle),
                              ),
                            ),
                          ),
                        ],
                      ),
                    ),
                  );
                },
              ),
            ),
          ],
        ),
      ),
    );
  }
}
