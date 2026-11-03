import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'terminal_shortcut_combo_editor.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_sizes.dart';
import '../../../../common/constants/app_theme.dart';
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
  List<TerminalShortcut> _combos = [];
  bool _enabled = true;
  bool _isSaving = false;
  String? _error;

  void _loadDraft(TerminalShortcutProfile profile) {
    _enabled = profile.enabled;
    _combos = [...profile.combos];
    _selected = profile.ids.toSet();
    _order = [
      ...profile.ids,
      ...profile.combos
          .map((key) => key.id)
          .where((id) => !_selected.contains(id)),
    ];
  }

  Future<void> _editCombo([TerminalShortcut? existing]) async {
    final combo = await showModalBottomSheet<TerminalShortcut>(
      context: context,
      isScrollControlled: true,
      useSafeArea: true,
      builder: (_) => TerminalShortcutComboEditor(combo: existing),
    );
    if (!mounted || combo == null) return;
    setState(() {
      final index = _combos.indexWhere((entry) => entry.id == combo.id);
      if (index < 0) {
        _combos.add(combo);
        _order!.insert(0, combo.id);
        _selected.add(combo.id);
      } else {
        _combos[index] = combo;
      }
    });
  }

  void _deleteCombo(String id) => setState(() {
    _combos.removeWhere((combo) => combo.id == id);
    _order!.remove(id);
    _selected.remove(id);
  });

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
              combos: _combos,
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
    final keys = {for (final key in _combos) key.id: key};
    return SizedBox(
      height: MediaQuery.sizeOf(context).height * .8,
      child: Column(
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(20, 16, 8, 0),
            child: Row(
              children: [
                IconButton(
                  tooltip: '닫기',
                  onPressed: _isSaving ? null : () => Navigator.pop(context),
                  icon: const Icon(Icons.close),
                ),
                Expanded(
                  child: Text(
                    '${widget.sessionType.name} 단축키',
                    style: Theme.of(context).textTheme.titleLarge,
                  ),
                ),
                IconButton(
                  tooltip: '콤보 추가',
                  onPressed:
                      _isSaving ||
                          _order == null ||
                          _combos.length >= maxTerminalCombos
                      ? null
                      : _editCombo,
                  icon: const Icon(Icons.add_circle_outline),
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
            Expanded(
              child: Material(
                type: MaterialType.transparency,
                clipBehavior: Clip.hardEdge,
                child: ReorderableListView.builder(
                  header: Column(
                    children: [
                      Padding(
                        padding: const EdgeInsets.fromLTRB(
                          AppSizes.p16,
                          AppSizes.p12,
                          AppSizes.p16,
                          0,
                        ),
                        child: Container(
                          width: double.infinity,
                          padding: const EdgeInsets.all(AppSizes.p12),
                          decoration: BoxDecoration(
                            color: AppColors.buttonNeutral,
                            borderRadius: BorderRadius.circular(
                              AppSizes.radiusLg,
                            ),
                          ),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Text(
                                '키보드 미리보기',
                                style: Theme.of(context).textTheme.bodySmall,
                              ),
                              const SizedBox(height: AppSizes.p8),
                              if (!_enabled)
                                const Text('단축키 바가 숨겨집니다')
                              else if (_selected.isEmpty)
                                const Text('아래에서 표시할 콤보를 선택하세요')
                              else
                                SingleChildScrollView(
                                  scrollDirection: Axis.horizontal,
                                  child: Row(
                                    children: [
                                      for (final id in _order!.where(
                                        _selected.contains,
                                      ))
                                        Padding(
                                          padding: const EdgeInsets.only(
                                            right: AppSizes.p8,
                                          ),
                                          child: Chip(
                                            label: Text(
                                              keys[id]!.label,
                                              style: monoStyle(
                                                color: AppColors.textPrimary,
                                              ),
                                            ),
                                          ),
                                        ),
                                    ],
                                  ),
                                ),
                            ],
                          ),
                        ),
                      ),
                      SwitchListTile(
                        title: const Text('단축키 바 표시'),
                        value: _enabled,
                        onChanged: _isSaving
                            ? null
                            : (value) => setState(() => _enabled = value),
                      ),
                      Align(
                        alignment: Alignment.centerLeft,
                        child: Padding(
                          padding: const EdgeInsets.symmetric(horizontal: 12),
                          child: TextButton.icon(
                            onPressed:
                                _isSaving || _combos.length >= maxTerminalCombos
                                ? null
                                : _editCombo,
                            icon: const Icon(Icons.add),
                            label: Text(
                              '콤보 추가 (${_combos.length}/$maxTerminalCombos)',
                            ),
                          ),
                        ),
                      ),
                      const Padding(
                        padding: EdgeInsets.symmetric(
                          horizontal: 20,
                          vertical: 8,
                        ),
                        child: Text(
                          '표시할 콤보를 선택하고 오른쪽 손잡이를 끌어 순서를 바꾸세요. 이 기기의 같은 세션 종류에 적용됩니다.',
                          style: TextStyle(color: AppColors.textSecondary),
                        ),
                      ),
                    ],
                  ),
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
                    return Padding(
                      key: ValueKey(key.id),
                      padding: const EdgeInsets.symmetric(
                        horizontal: AppSizes.p12,
                        vertical: AppSizes.p4,
                      ),
                      child: CheckboxListTile(
                        tileColor: AppColors.buttonNeutral,
                        shape: RoundedRectangleBorder(
                          borderRadius: BorderRadius.circular(
                            AppSizes.radiusLg,
                          ),
                        ),
                        value: _selected.contains(key.id),
                        title: Wrap(
                          spacing: AppSizes.p8,
                          crossAxisAlignment: WrapCrossAlignment.center,
                          children: [
                            Text(
                              key.label,
                              style: monoStyle(color: AppColors.textPrimary),
                            ),
                            if (key.chord != null)
                              const Text(
                                '콤보',
                                style: TextStyle(
                                  fontSize: 11,
                                  color: AppColors.textSecondary,
                                ),
                              ),
                          ],
                        ),
                        subtitle: Text(
                          key.description,
                          maxLines: 2,
                          overflow: TextOverflow.ellipsis,
                        ),
                        controlAffinity: ListTileControlAffinity.leading,
                        contentPadding: const EdgeInsets.symmetric(
                          horizontal: 8,
                        ),
                        secondary: Row(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            if (key.chord != null)
                              PopupMenuButton<String>(
                                tooltip: '${key.label} 콤보 동작',
                                enabled: !_isSaving,
                                onSelected: (action) {
                                  if (action == 'edit') {
                                    _editCombo(key);
                                  } else {
                                    _deleteCombo(key.id);
                                  }
                                },
                                itemBuilder: (_) => const [
                                  PopupMenuItem(
                                    value: 'edit',
                                    child: Text('콤보 편집'),
                                  ),
                                  PopupMenuItem(
                                    value: 'delete',
                                    child: Text('콤보 삭제'),
                                  ),
                                ],
                              ),
                            ReorderableDragStartListener(
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
                          ],
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
                      ),
                    );
                  },
                ),
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
                    Expanded(
                      child: TextButton(
                        onPressed: _isSaving
                            ? null
                            : () => setState(
                                () => _loadDraft(
                                  TerminalShortcutProfile.defaults(),
                                ),
                              ),
                        child: const Text('기본값 복원'),
                      ),
                    ),
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
