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
/// 기본 세팅과 내 콤보를 바에 보이는 순서대로 한 목록에 보여 준다. 고친 내용은 초안으로만 들고 있다가
/// "적용"을 눌러야 저장한다. 닫으면 초안은 버려진다.
class TerminalShortcutSettingsPage extends ConsumerStatefulWidget {
  const TerminalShortcutSettingsPage({required this.sessionType, super.key});

  final SessionType sessionType;

  @override
  ConsumerState<TerminalShortcutSettingsPage> createState() =>
      _TerminalShortcutSettingsPageState();
}

enum _ImportMode { merge, overwrite }

class _TerminalShortcutSettingsPageState
    extends ConsumerState<TerminalShortcutSettingsPage> {
  List<String>? _order;
  Set<String> _selected = {};
  List<TerminalShortcut> _combos = [];
  Set<String> _removedDefaultIds = {};
  bool _enabled = true;
  bool _isSaving = false;
  String? _error;

  bool get _canAddCombo => !_isSaving && _combos.length < maxTerminalCombos;

  void _loadDraft(TerminalShortcutProfile profile) {
    final visibleIds = profile
        .visibleShortcuts(widget.sessionType)
        .map((shortcut) => shortcut.id)
        .toList();
    _enabled = profile.enabled;
    _combos = [...profile.combos];
    _removedDefaultIds = profile.removedDefaultIds.toSet();
    _selected = visibleIds.toSet();
    _order = [
      ...visibleIds,
      for (final shortcut in [
        ...profile.remainingDefaults(widget.sessionType),
        ...profile.combos,
      ])
        if (!_selected.contains(shortcut.id)) shortcut.id,
    ];
  }

  TerminalShortcutProfile _draftProfile() => TerminalShortcutProfile(
    enabled: _enabled,
    combos: _combos,
    ids: _order!.where(_selected.contains).toList(),
    removedDefaultIds: _removedDefaultIds.toList(),
  );

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
        _combos.add(combo);
        _order!.add(combo.id);
        _selected.add(combo.id);
      } else {
        _combos[index] = combo;
      }
    });
  }

  void _deleteShortcut(TerminalShortcut shortcut) => setState(() {
    if (shortcut.isCombo) {
      _combos.removeWhere((combo) => combo.id == shortcut.id);
    } else {
      _removedDefaultIds.add(shortcut.id);
    }
    _order!.remove(shortcut.id);
    _selected.remove(shortcut.id);
  });

  /// 지운 기본 세팅만 바에서 뺀 채로 목록 끝에 되살린다. 내 콤보는 건드리지 않는다
  Future<void> _restoreDefaults() async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('기본값을 복원할까요?'),
        content: Text(
          '지운 기본 세팅 ${_removedDefaultIds.length}개가 다시 생겨요. 내 콤보는 그대로예요.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('취소'),
          ),
          TextButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('복원'),
          ),
        ],
      ),
    );
    if (confirmed != true || !mounted) return;
    setState(() {
      _order!.addAll(
        terminalShortcuts(widget.sessionType)
            .map((shortcut) => shortcut.id)
            .where(_removedDefaultIds.contains),
      );
      _removedDefaultIds.clear();
    });
  }

  /// 다른 세션 종류의 저장된 설정을 합치거나 덮어써서 초안에 넣는다
  Future<void> _importFrom(
    SessionType source,
    TerminalShortcutProfile sourceProfile,
  ) async {
    final mode = await showModalBottomSheet<_ImportMode>(
      context: context,
      builder: (_) => _ImportModeSheet(source: source),
    );
    if (mode == null || !mounted) return;
    final draft = _draftProfile();
    final newComboIdPrefix = 'combo-${DateTime.now().microsecondsSinceEpoch}';
    final imported = switch (mode) {
      _ImportMode.merge => draft.mergedWith(
        sourceProfile,
        widget.sessionType,
        newComboIdPrefix: newComboIdPrefix,
      ),
      _ImportMode.overwrite => draft.overwrittenBy(
        sourceProfile,
        widget.sessionType,
        newComboIdPrefix: newComboIdPrefix,
      ),
    };
    final addedCount = imported.ids.length - draft.ids.length;
    setState(() => _loadDraft(imported));
    final message = switch (mode) {
      _ImportMode.overwrite => '${source.name} 설정으로 덮어썼어요',
      _ when addedCount == 0 => '가져올 새 단축키가 없어요',
      _ => '${source.name} 설정에서 $addedCount개를 가져왔어요',
    };
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(SnackBar(content: Text(message)));
  }

  Future<void> _save() async {
    setState(() {
      _isSaving = true;
      _error = null;
    });
    try {
      await ref
          .read(terminalShortcutControllerProvider.notifier)
          .save(widget.sessionType, _draftProfile());
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
              onPressed: _isSaving || _removedDefaultIds.isEmpty
                  ? null
                  : _restoreDefaults,
              child: const Text('기본값 복원'),
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
    final shortcutsById = {
      for (final shortcut in [
        ...terminalShortcuts(widget.sessionType),
        ..._combos,
      ])
        shortcut.id: shortcut,
    };
    final visibleShortcuts = [
      for (final id in _order!.where(_selected.contains)) shortcutsById[id]!,
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
                subtitle: const Text('터미널 아래에 단축키 버튼을 보여요'),
                value: _enabled,
                onChanged: _isSaving
                    ? null
                    : (value) => setState(() => _enabled = value),
              ),
            ],
          ),
          const SettingsSectionLabel('단축키 바 미리보기'),
          _ShortcutBarPreview(enabled: _enabled, shortcuts: visibleShortcuts),
          SettingsSectionLabel(
            '바에 보일 단축키',
            caption:
                '체크한 단축키가 바에 보여요. 손잡이를 끌어 순서를 바꿔요 · 내 콤보 ${_combos.length}/$maxTerminalCombos',
          ),
        ],
      ),
      footer: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _AddComboButton(onPressed: _canAddCombo ? _editCombo : null),
          const SettingsSectionLabel(
            '다른 세션에서 가져오기',
            caption: '다른 세션 종류의 기본 세팅 표시와 콤보를 복사해요',
          ),
          SettingsCard(
            children: [
              for (final source in SessionType.values)
                if (source != widget.sessionType)
                  _ImportSourceTile(
                    source: source,
                    profile:
                        profiles[source] ?? TerminalShortcutProfile.defaults(),
                    onImport: _isSaving
                        ? null
                        : () => _importFrom(
                            source,
                            profiles[source] ??
                                TerminalShortcutProfile.defaults(),
                          ),
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
        final shortcut = shortcutsById[_order![index]]!;
        return _ShortcutTile(
          key: ValueKey(shortcut.id),
          index: index,
          shortcut: shortcut,
          isSelected: _selected.contains(shortcut.id),
          isSaving: _isSaving,
          onSelected: (selected) => setState(() {
            if (selected) {
              _selected.add(shortcut.id);
            } else {
              _selected.remove(shortcut.id);
            }
          }),
          onEdit: shortcut.isCombo ? () => _editCombo(shortcut) : null,
          onDelete: () => _deleteShortcut(shortcut),
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
  const _ShortcutBarPreview({required this.enabled, required this.shortcuts});

  final bool enabled;
  final List<TerminalShortcut> shortcuts;

  @override
  Widget build(BuildContext context) {
    final emptyMessage = !enabled
        ? '단축키 바가 숨겨져요'
        : shortcuts.isEmpty
        ? '아래에서 바에 보일 단축키를 체크해 주세요'
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
                  for (final shortcut in shortcuts)
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
                        shortcut.label,
                        style: monoStyle(color: AppColors.textPrimary),
                      ),
                    ),
                ],
              ),
            ),
    );
  }
}

class _ShortcutTile extends StatelessWidget {
  const _ShortcutTile({
    required this.index,
    required this.shortcut,
    required this.isSelected,
    required this.isSaving,
    required this.onSelected,
    required this.onEdit,
    required this.onDelete,
    super.key,
  });

  final int index;
  final TerminalShortcut shortcut;
  final bool isSelected;
  final bool isSaving;
  final ValueChanged<bool> onSelected;

  /// 콤보만 편집할 수 있다. 기본 세팅이면 null이다
  final VoidCallback? onEdit;
  final VoidCallback onDelete;

  @override
  Widget build(BuildContext context) {
    final kindLabel = shortcut.isCombo ? '콤보' : '기본 세팅';
    return Padding(
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
          title: Row(
            children: [
              Flexible(
                child: Text(shortcut.label, overflow: TextOverflow.ellipsis),
              ),
              const SizedBox(width: AppSizes.p8),
              _ShortcutKindTag(isCombo: shortcut.isCombo),
            ],
          ),
          subtitle: Text(
            shortcut.description,
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
            style: monoStyle(),
          ),
          secondary: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              PopupMenuButton<VoidCallback>(
                tooltip: '${shortcut.label} $kindLabel 동작',
                enabled: !isSaving,
                onSelected: (action) => action(),
                itemBuilder: (_) => [
                  if (onEdit case final onEdit?)
                    PopupMenuItem(value: onEdit, child: const Text('콤보 편집')),
                  PopupMenuItem(value: onDelete, child: Text('$kindLabel 삭제')),
                ],
              ),
              ReorderableDragStartListener(
                index: index,
                enabled: !isSaving,
                child: Semantics(
                  label: '${shortcut.label} 순서 변경',
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
}

/// 목록에 섞인 항목이 기본 세팅인지 내 콤보인지 알려 주는 작은 태그
class _ShortcutKindTag extends StatelessWidget {
  const _ShortcutKindTag({required this.isCombo});

  final bool isCombo;

  @override
  Widget build(BuildContext context) => Container(
    padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
    decoration: BoxDecoration(
      color: isCombo ? AppColors.brandSubtle : AppColors.buttonNeutral,
      borderRadius: BorderRadius.circular(AppSizes.radiusMd),
    ),
    child: Text(
      isCombo ? '콤보' : '기본',
      style: const TextStyle(fontSize: 11, color: AppColors.textSecondary),
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
    required this.profile,
    required this.onImport,
  });

  final SessionType source;
  final TerminalShortcutProfile profile;
  final VoidCallback? onImport;

  @override
  Widget build(BuildContext context) {
    final shownDefaultCount = profile
        .visibleShortcuts(source)
        .where((shortcut) => !shortcut.isCombo)
        .length;
    final hasSettings = shownDefaultCount > 0 || profile.combos.isNotEmpty;
    return Semantics(
      identifier: 'combo-import-${source.name}',
      child: ListTile(
        enabled: hasSettings && onImport != null,
        onTap: onImport,
        title: Text('${source.name} 설정'),
        subtitle: Text(
          hasSettings
              ? '바에 띄운 기본 세팅 $shownDefaultCount개 · 콤보 ${profile.combos.length}개'
              : '바에 띄운 단축키가 없어요',
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
        ),
        trailing: Text(
          hasSettings ? '가져오기' : '',
          style: const TextStyle(
            color: AppColors.brandPrimary,
            fontWeight: FontWeight.w600,
          ),
        ),
      ),
    );
  }
}

class _ImportModeSheet extends StatelessWidget {
  const _ImportModeSheet({required this.source});

  final SessionType source;

  @override
  Widget build(BuildContext context) => SafeArea(
    child: Padding(
      padding: const EdgeInsets.symmetric(vertical: AppSizes.p16),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: AppSizes.p24),
            child: Text(
              '${source.name} 설정을 어떻게 가져올까요?',
              style: const TextStyle(fontSize: 17, fontWeight: FontWeight.w700),
            ),
          ),
          const SizedBox(height: AppSizes.p8),
          ListTile(
            title: const Text('합치기'),
            subtitle: const Text('같은 콤보는 건너뛰고 지금 설정에 더해요'),
            onTap: () => Navigator.pop(context, _ImportMode.merge),
          ),
          ListTile(
            title: const Text('덮어쓰기'),
            subtitle: Text('지금 설정을 ${source.name} 설정과 똑같이 바꿔요'),
            onTap: () => Navigator.pop(context, _ImportMode.overwrite),
          ),
        ],
      ),
    ),
  );
}
