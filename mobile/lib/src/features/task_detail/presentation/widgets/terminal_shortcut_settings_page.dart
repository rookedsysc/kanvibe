import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_sizes.dart';
import '../../../../common/constants/app_theme.dart';
import '../../../task_create/domain/session_type.dart';
import '../../application/terminal_shortcut_controller.dart';
import '../../domain/terminal_shortcut.dart';
import 'terminal_shortcut_combo_editor.dart';
import 'terminal_shortcut_settings_layout.dart';

/// 세션 종류 하나의 단축키 바를 고치는 전체 화면 설정 페이지.
///
/// 고친 내용은 초안으로만 들고 있다가 "적용"을 눌러야 저장한다. 닫으면 초안은 버려진다.
class TerminalShortcutSettingsPage extends ConsumerStatefulWidget {
  const TerminalShortcutSettingsPage({required this.sessionType, super.key});

  final SessionType sessionType;

  @override
  ConsumerState<TerminalShortcutSettingsPage> createState() =>
      _TerminalShortcutSettingsPageState();
}

class _TerminalShortcutSettingsPageState
    extends ConsumerState<TerminalShortcutSettingsPage> {
  List<String>? _order;
  Set<String> _selected = {};
  List<TerminalShortcut> _combos = [];
  bool _enabled = true;
  bool _isSaving = false;
  String? _error;

  bool get _canAddCombo => !_isSaving && _combos.length < maxTerminalCombos;

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
    final combo = await Navigator.of(context).push<TerminalShortcut>(
      MaterialPageRoute(
        builder: (_) => TerminalShortcutComboEditor(combo: existing),
      ),
    );
    if (!mounted || combo == null) return;
    setState(() {
      final index = _combos.indexWhere((entry) => entry.id == combo.id);
      if (index < 0) {
        _addVisibleCombo(combo);
      } else {
        _combos[index] = combo;
      }
    });
  }

  void _addVisibleCombo(TerminalShortcut combo) {
    _combos.add(combo);
    _order!.add(combo.id);
    _selected.add(combo.id);
  }

  void _deleteCombo(String id) => setState(() {
    _combos.removeWhere((combo) => combo.id == id);
    _order!.remove(id);
    _selected.remove(id);
  });

  /// 다른 세션 종류의 콤보를 복사해 넣는다. 이미 같은 키 조합이 있으면 건너뛰고, 최대 개수까지만 넣는다
  void _importCombos(SessionType source, List<TerminalShortcut> sourceCombos) {
    final importedAt = DateTime.now().microsecondsSinceEpoch;
    var importedCount = 0;
    setState(() {
      for (final combo in sourceCombos) {
        if (_combos.length >= maxTerminalCombos) break;
        final chord = combo.chord!;
        final isDuplicate = _combos.any(
          (existing) => existing.chord!.isSameChord(chord),
        );
        if (isDuplicate) continue;
        _addVisibleCombo(
          TerminalShortcut.combo(
            'combo-$importedAt-$importedCount',
            combo.label,
            chord,
          ),
        );
        importedCount++;
      }
    });
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(
        SnackBar(
          content: Text(
            importedCount == 0
                ? '가져올 새 콤보가 없어요'
                : '${source.name} 콤보 $importedCount개를 가져왔어요',
          ),
        ),
      );
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
              combos: _combos,
              ids: _order!.where(_selected.contains).toList(),
            ),
          );
      if (mounted) Navigator.pop(context);
    } catch (_) {
      if (mounted) {
        setState(() {
          _isSaving = false;
          _error = '저장하지 못했어요. 다시 시도해 주세요.';
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
    return Scaffold(
      appBar: AppBar(
        leading: IconButton(
          tooltip: '닫기',
          onPressed: _isSaving ? null : () => Navigator.pop(context),
          icon: const Icon(Icons.close),
        ),
        actions: [
          if (_order != null)
            TextButton(
              onPressed: _isSaving
                  ? null
                  : () => setState(
                      () => _loadDraft(TerminalShortcutProfile.defaults()),
                    ),
              child: const Text('초기화'),
            ),
        ],
      ),
      body: _order == null
          ? _LoadingState(
              hasError: settings.hasError,
              onRetry: () => ref.invalidate(terminalShortcutControllerProvider),
            )
          : _buildDraftList(profiles ?? const {}),
      // 하단 바에 두어야 가져오기 안내 스낵바가 적용 버튼을 덮지 않고 그 위에 뜬다
      bottomNavigationBar: _order == null
          ? null
          : SafeArea(
              top: false,
              child: SettingsBottomAction(
                error: _error,
                label: _isSaving ? '저장 중…' : '적용',
                onPressed: _isSaving ? null : _save,
              ),
            ),
    );
  }

  Widget _buildDraftList(Map<SessionType, TerminalShortcutProfile> profiles) {
    final combosById = {for (final combo in _combos) combo.id: combo};
    final visibleCombos = [
      for (final id in _order!.where(_selected.contains)) combosById[id]!,
    ];
    return ReorderableListView.builder(
      padding: const EdgeInsets.symmetric(horizontal: AppSizes.p24),
      buildDefaultDragHandles: false,
      header: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          SettingsPageTitle(
            title: '${widget.sessionType.name} 단축키',
            subtitle: '이 기기의 ${widget.sessionType.name} 세션에 적용돼요',
          ),
          SettingsCard(
            children: [
              SwitchListTile(
                title: const Text('단축키 바 표시'),
                subtitle: const Text('터미널 아래에 콤보 버튼을 보여요'),
                value: _enabled,
                onChanged: _isSaving
                    ? null
                    : (value) => setState(() => _enabled = value),
              ),
            ],
          ),
          const SettingsSectionLabel('단축키 바 미리보기'),
          _ShortcutBarPreview(enabled: _enabled, combos: visibleCombos),
          SettingsSectionLabel(
            '내 콤보 ${_combos.length}/$maxTerminalCombos',
            caption: '체크한 콤보가 바에 보여요. 손잡이를 끌어 순서를 바꿔요',
          ),
        ],
      ),
      footer: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _AddComboButton(onPressed: _canAddCombo ? _editCombo : null),
          const SettingsSectionLabel(
            '다른 세션에서 가져오기',
            caption: '다른 세션 종류에 만든 콤보를 그대로 복사해요',
          ),
          SettingsCard(
            children: [
              for (final source in SessionType.values)
                if (source != widget.sessionType)
                  _ImportSourceTile(
                    source: source,
                    combos: profiles[source]?.combos ?? const [],
                    onImport: _canAddCombo
                        ? () => _importCombos(
                            source,
                            profiles[source]?.combos ?? const [],
                          )
                        : null,
                  ),
            ],
          ),
          const SizedBox(height: AppSizes.p24),
        ],
      ),
      itemCount: _order!.length,
      onReorderItem: (oldIndex, newIndex) {
        if (_isSaving) return;
        setState(() => _order!.insert(newIndex, _order!.removeAt(oldIndex)));
      },
      itemBuilder: (context, index) {
        final combo = combosById[_order![index]]!;
        return _ComboTile(
          key: ValueKey(combo.id),
          index: index,
          combo: combo,
          isSelected: _selected.contains(combo.id),
          isSaving: _isSaving,
          onSelected: (selected) => setState(() {
            if (selected) {
              _selected.add(combo.id);
            } else {
              _selected.remove(combo.id);
            }
          }),
          onEdit: () => _editCombo(combo),
          onDelete: () => _deleteCombo(combo.id),
        );
      },
    );
  }
}

class _LoadingState extends StatelessWidget {
  const _LoadingState({required this.hasError, required this.onRetry});

  final bool hasError;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) => Center(
    child: hasError
        ? TextButton(
            onPressed: onRetry,
            child: const Text('설정을 불러오지 못했어요 · 다시 시도'),
          )
        : const CircularProgressIndicator(),
  );
}

class _ShortcutBarPreview extends StatelessWidget {
  const _ShortcutBarPreview({required this.enabled, required this.combos});

  final bool enabled;
  final List<TerminalShortcut> combos;

  @override
  Widget build(BuildContext context) {
    final emptyMessage = !enabled
        ? '단축키 바가 숨겨져요'
        : combos.isEmpty
        ? '아래에서 바에 보일 콤보를 체크해 주세요'
        : null;
    return Container(
      height: 56,
      padding: const EdgeInsets.symmetric(horizontal: AppSizes.p12),
      alignment: Alignment.centerLeft,
      decoration: BoxDecoration(
        color: AppColors.terminalChrome,
        borderRadius: BorderRadius.circular(AppSizes.radiusCard),
        border: Border.all(color: AppColors.borderDefault),
      ),
      child: emptyMessage != null
          ? Text(
              emptyMessage,
              style: const TextStyle(color: AppColors.textMuted),
            )
          : SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              child: Row(
                children: [
                  for (final combo in combos)
                    Container(
                      margin: const EdgeInsets.only(right: AppSizes.p8),
                      padding: const EdgeInsets.symmetric(
                        horizontal: AppSizes.p12,
                        vertical: AppSizes.p8,
                      ),
                      decoration: BoxDecoration(
                        color: AppColors.buttonNeutral,
                        borderRadius: BorderRadius.circular(AppSizes.radiusMd),
                      ),
                      child: Text(
                        combo.label,
                        style: monoStyle(color: AppColors.textPrimary),
                      ),
                    ),
                ],
              ),
            ),
    );
  }
}

class _ComboTile extends StatelessWidget {
  const _ComboTile({
    required this.index,
    required this.combo,
    required this.isSelected,
    required this.isSaving,
    required this.onSelected,
    required this.onEdit,
    required this.onDelete,
    super.key,
  });

  final int index;
  final TerminalShortcut combo;
  final bool isSelected;
  final bool isSaving;
  final ValueChanged<bool> onSelected;
  final VoidCallback onEdit;
  final VoidCallback onDelete;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.only(bottom: AppSizes.p8),
    child: Material(
      color: AppColors.bgSurface,
      clipBehavior: Clip.antiAlias,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(AppSizes.radiusCard),
      ),
      child: CheckboxListTile(
        value: isSelected,
        checkboxShape: const CircleBorder(),
        controlAffinity: ListTileControlAffinity.leading,
        contentPadding: const EdgeInsets.only(left: AppSizes.p8),
        title: Text(combo.label),
        subtitle: Text(
          combo.description,
          maxLines: 2,
          overflow: TextOverflow.ellipsis,
          style: monoStyle(),
        ),
        secondary: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            PopupMenuButton<VoidCallback>(
              tooltip: '${combo.label} 콤보 동작',
              enabled: !isSaving,
              onSelected: (action) => action(),
              itemBuilder: (_) => [
                PopupMenuItem(value: onEdit, child: const Text('콤보 편집')),
                PopupMenuItem(value: onDelete, child: const Text('콤보 삭제')),
              ],
            ),
            ReorderableDragStartListener(
              index: index,
              enabled: !isSaving,
              child: Semantics(
                label: '${combo.label} 순서 변경',
                child: const SizedBox.square(
                  dimension: AppSizes.minTouchTarget,
                  child: Icon(Icons.drag_handle, color: AppColors.textMuted),
                ),
              ),
            ),
          ],
        ),
        onChanged: isSaving ? null : (selected) => onSelected(selected!),
      ),
    ),
  );
}

class _AddComboButton extends StatelessWidget {
  const _AddComboButton({required this.onPressed});

  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) => Semantics(
    identifier: 'combo-add',
    child: TextButton.icon(
      onPressed: onPressed,
      style: TextButton.styleFrom(
        minimumSize: const Size.fromHeight(56),
        backgroundColor: AppColors.bgSurface,
        foregroundColor: AppColors.brandPrimary,
        textStyle: const TextStyle(fontSize: 15, fontWeight: FontWeight.w600),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(AppSizes.radiusCard),
        ),
      ),
      icon: const Icon(Icons.add),
      label: const Text('콤보 추가'),
    ),
  );
}

class _ImportSourceTile extends StatelessWidget {
  const _ImportSourceTile({
    required this.source,
    required this.combos,
    required this.onImport,
  });

  final SessionType source;
  final List<TerminalShortcut> combos;
  final VoidCallback? onImport;

  @override
  Widget build(BuildContext context) {
    final hasCombos = combos.isNotEmpty;
    return Semantics(
      identifier: 'combo-import-${source.name}',
      child: ListTile(
        enabled: hasCombos && onImport != null,
        onTap: onImport,
        title: Text('${source.name} 콤보'),
        subtitle: Text(
          hasCombos
              ? combos.map((combo) => combo.label).join(', ')
              : '아직 만든 콤보가 없어요',
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
        ),
        trailing: Text(
          hasCombos ? '${combos.length}개 가져오기' : '',
          style: const TextStyle(
            color: AppColors.brandPrimary,
            fontWeight: FontWeight.w600,
          ),
        ),
      ),
    );
  }
}
