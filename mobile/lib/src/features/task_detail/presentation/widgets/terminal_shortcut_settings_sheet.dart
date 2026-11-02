import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../task_create/domain/session_type.dart';
import '../../application/terminal_shortcut_controller.dart';
import '../../domain/terminal_shortcut.dart';

class TerminalShortcutSettingsSheet extends ConsumerStatefulWidget {
  const TerminalShortcutSettingsSheet({required this.sessionType, super.key});

  final SessionType sessionType;

  @override
  ConsumerState<TerminalShortcutSettingsSheet> createState() =>
      _TerminalShortcutSettingsSheetState();
}

class _TerminalShortcutSettingsSheetState
    extends ConsumerState<TerminalShortcutSettingsSheet> {
  List<String>? _order;
  Set<String> _selected = {};
  bool _enabled = true;
  bool _isSaving = false;
  String? _error;

  void _loadDraft(TerminalShortcutProfile profile) {
    _enabled = profile.enabled;
    _selected = profile.ids.toSet();
    _order = [
      ...profile.ids,
      ...terminalShortcuts(widget.sessionType)
          .map((key) => key.id)
          .where((id) => !_selected.contains(id)),
    ];
  }

  Future<void> _save() async {
    setState(() {
      _isSaving = true;
      _error = null;
    });
    try {
      await ref
          .read(terminalShortcutControllerProvider.notifier)
          .save(
            widget.sessionType,
            TerminalShortcutProfile(
              enabled: _enabled,
              ids: _order!.where(_selected.contains).toList(),
            ),
          );
      if (mounted) Navigator.pop(context);
    } catch (_) {
      if (mounted) {
        setState(() {
          _isSaving = false;
          _error = '저장하지 못했습니다. 다시 시도해 주세요.';
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final settings = ref.watch(terminalShortcutControllerProvider);
    final profiles = settings.asData?.value;
    if (_order == null && profiles != null) {
      _loadDraft(profiles[widget.sessionType]!);
    }
    final keys = {
      for (final key in terminalShortcuts(widget.sessionType)) key.id: key,
    };
    return SizedBox(
      height: MediaQuery.sizeOf(context).height * .8,
      child: Column(
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(20, 16, 8, 0),
            child: Row(
              children: [
                Expanded(
                  child: Text(
                    '${widget.sessionType.name} 단축키',
                    style: Theme.of(context).textTheme.titleLarge,
                  ),
                ),
                IconButton(
                  tooltip: '닫기',
                  onPressed: _isSaving ? null : () => Navigator.pop(context),
                  icon: const Icon(Icons.close),
                ),
              ],
            ),
          ),
          if (_order == null)
            Expanded(
              child: Center(
                child: settings.hasError
                    ? TextButton(
                        onPressed: () =>
                            ref.invalidate(terminalShortcutControllerProvider),
                        child: const Text('설정을 불러오지 못했습니다 · 다시 시도'),
                      )
                    : const CircularProgressIndicator(),
              ),
            )
          else ...[
            SwitchListTile(
              title: const Text('단축키 바 표시'),
              value: _enabled,
              onChanged: _isSaving
                  ? null
                  : (value) => setState(() => _enabled = value),
            ),
            const Padding(
              padding: EdgeInsets.symmetric(horizontal: 20, vertical: 8),
              child: Text(
                '표시할 키를 선택하고 오른쪽 손잡이를 끌어 순서를 바꾸세요. 이 기기의 같은 세션 종류에 적용됩니다.',
                style: TextStyle(color: AppColors.textSecondary),
              ),
            ),
            Expanded(
              child: ReorderableListView.builder(
                itemCount: _order!.length,
                buildDefaultDragHandles: false,
                onReorderItem: (oldIndex, newIndex) {
                  if (_isSaving) return;
                  setState(() {
                    _order!.insert(newIndex, _order!.removeAt(oldIndex));
                  });
                },
                itemBuilder: (context, index) {
                  final key = keys[_order![index]]!;
                  return CheckboxListTile(
                    key: ValueKey(key.id),
                    value: _selected.contains(key.id),
                    title: Text(key.label),
                    subtitle: Text(key.description),
                    controlAffinity: ListTileControlAffinity.leading,
                    contentPadding: const EdgeInsets.symmetric(horizontal: 8),
                    secondary: ReorderableDragStartListener(
                      index: index,
                      enabled: !_isSaving,
                      child: Semantics(
                        label: '${key.label} 순서 변경',
                        child: const SizedBox.square(
                          dimension: 48,
                          child: Icon(Icons.drag_handle),
                        ),
                      ),
                    ),
                    onChanged: _isSaving
                        ? null
                        : (selected) => setState(() {
                            if (selected == true) {
                              _selected.add(key.id);
                            } else {
                              _selected.remove(key.id);
                            }
                          }),
                  );
                },
              ),
            ),
            if (_error != null)
              Padding(
                padding: const EdgeInsets.all(12),
                child: Text(
                  _error!,
                  style: const TextStyle(color: AppColors.statusError),
                ),
              ),
            SafeArea(
              top: false,
              child: Padding(
                padding: const EdgeInsets.all(12),
                child: Row(
                  children: [
                    TextButton(
                      onPressed: _isSaving
                          ? null
                          : () => setState(
                              () => _loadDraft(
                                TerminalShortcutProfile.defaults(
                                  widget.sessionType,
                                ),
                              ),
                            ),
                      child: const Text('기본값 복원'),
                    ),
                    const Spacer(),
                    SizedBox(
                      width: 128,
                      child: FilledButton(
                        onPressed: _isSaving ? null : _save,
                        child: Text(_isSaving ? '저장 중…' : '적용'),
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ],
        ],
      ),
    );
  }
}
