import '../../../../common/l10n/app_localizations.dart';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_sizes.dart';
import '../../../../common/constants/app_theme.dart';
import '../../domain/terminal_shortcut.dart';
import 'terminal_shortcut_settings_layout.dart';

/// 콤보 하나를 만들거나 고치는 전체 화면 편집기.
///
/// 위에서 특수 키(보조 키 포함)를 여러 개 고르고, 아래 입력란에 일반 키를 여러 글자 친다.
/// 고른 키는 모두 한 번에 누르는 조합이 되며, 저장하면 완성된 콤보를 돌려준다.
class TerminalShortcutComboEditor extends StatefulWidget {
  const TerminalShortcutComboEditor({this.combo, super.key});

  final TerminalShortcut? combo;

  @override
  State<TerminalShortcutComboEditor> createState() =>
      _TerminalShortcutComboEditorState();
}

class _TerminalShortcutComboEditorState
    extends State<TerminalShortcutComboEditor> {
  late final _label = TextEditingController(text: widget.combo?.label ?? '');
  late final _characters = TextEditingController(
    text: _initialChord.keys.where(TerminalShortcutChord.isCharacterKey).join(),
  );
  late bool _ctrl = _initialChord.ctrl;
  late bool _alt = _initialChord.alt;
  late bool _shift = _initialChord.shift;
  late final List<String> _specialKeys = _initialChord.keys
      .where((key) => !TerminalShortcutChord.isCharacterKey(key))
      .toList();
  String? _error;

  TerminalShortcutChord get _initialChord =>
      widget.combo?.chord ?? TerminalShortcutChord(keys: []);

  /// 같은 글자를 두 번 친 것은 한 번 누른 것으로 본다. 한 키를 동시에 두 번 누를 수는 없다
  TerminalShortcutChord get _chord {
    final typedKeys = _characters.text.characters.toSet();
    return TerminalShortcutChord(
      keys: [..._specialKeys, ...typedKeys],
      ctrl: _ctrl,
      alt: _alt,
      shift: _shift,
    );
  }

  @override
  void dispose() {
    _label.dispose();
    _characters.dispose();
    super.dispose();
  }

  void _update(VoidCallback change) => setState(() {
    change();
    _error = null;
  });

  void _toggleSpecialKey(String id) => _update(() {
    if (!_specialKeys.remove(id)) _specialKeys.add(id);
  });

  void _submit() {
    final label = _label.text.trim();
    final chord = _chord;
    if (label.isEmpty) {
      setState(() => _error = '콤보 이름을 입력해 주세요.');
      return;
    }
    if (!chord.isValid) {
      setState(
        () => _error = chord.keys.isEmpty
            ? '함께 누를 키를 하나 이상 골라 주세요.'
            : 'Ctrl과 함께 누를 글자는 영문이나 기호만 쓸 수 있어요.',
      );
      return;
    }
    Navigator.pop(
      context,
      TerminalShortcut.combo(
        widget.combo?.id ?? 'combo-${DateTime.now().microsecondsSinceEpoch}',
        label,
        chord,
      ),
    );
  }

  @override
  Widget build(BuildContext context) => Scaffold(
    appBar: AppBar(
      leading: IconButton(
        tooltip: context.tr('콤보 닫기'),
        onPressed: () => Navigator.pop(context),
        icon: Icon(Icons.close),
      ),
    ),
    body: SafeArea(
      top: false,
      child: Column(
        children: [
          Expanded(
            child: ListView(
              padding: EdgeInsets.fromLTRB(
                AppSizes.p24,
                0,
                AppSizes.p24,
                AppSizes.p24,
              ),
              children: [
                SettingsPageTitle(
                  title: widget.combo == null ? '콤보 추가' : '콤보 편집',
                  subtitle: '고른 키를 모두 한 번에 눌러요',
                ),
                _ChordPreview(chord: _chord),
                SettingsSectionLabel('이름'),
                Semantics(
                  identifier: 'combo-name',
                  child: TextField(
                    controller: _label,
                    maxLength: maxTerminalComboLabelLength,
                    decoration: InputDecoration(
                      labelText: context.tr('콤보 이름'),
                      hintText: context.tr('예: 좌우 분할'),
                    ),
                    onChanged: (_) => setState(() => _error = null),
                  ),
                ),
                SettingsSectionLabel('특수 키', caption: '여러 개를 골라 함께 누를 수 있어요'),
                _SpecialKeyPicker(
                  ctrl: _ctrl,
                  alt: _alt,
                  shift: _shift,
                  selectedKeys: _specialKeys,
                  onCtrl: (value) => _update(() => _ctrl = value),
                  onAlt: (value) => _update(() => _alt = value),
                  onShift: (value) => _update(() => _shift = value),
                  onSpecialKey: _toggleSpecialKey,
                ),
                SettingsSectionLabel('일반 키', caption: '입력한 글자마다 함께 누를 키가 돼요'),
                Semantics(
                  identifier: 'combo-key-input',
                  child: TextField(
                    controller: _characters,
                    autocorrect: false,
                    enableSuggestions: false,
                    style: monoStyle(
                      color: AppColors.textPrimary,
                      fontSize: 16,
                    ),
                    inputFormatters: [
                      LengthLimitingTextInputFormatter(maxTerminalChordKeys),
                    ],
                    decoration: InputDecoration(
                      labelText: context.tr('입력 키'),
                      hintText: context.tr('예: b, %, ab'),
                    ),
                    onChanged: (_) => _update(() {}),
                  ),
                ),
              ],
            ),
          ),
          SettingsBottomAction(error: _error, label: '저장', onPressed: _submit),
        ],
      ),
    ),
  );
}

class _ChordPreview extends StatelessWidget {
  const _ChordPreview({required this.chord});

  final TerminalShortcutChord chord;

  @override
  Widget build(BuildContext context) {
    final keyLabels = [
      if (chord.ctrl) 'Ctrl',
      if (chord.alt) 'Alt',
      if (chord.shift) 'Shift',
      for (final key in chord.keys) TerminalShortcutChord.keyLabel(key),
    ];
    return Semantics(
      identifier: 'combo-preview',
      label: chord.keys.isEmpty ? null : chord.label,
      child: Container(
        constraints: BoxConstraints(minHeight: 88),
        alignment: Alignment.center,
        padding: EdgeInsets.all(AppSizes.p16),
        decoration: BoxDecoration(
          color: AppColors.terminalBg,
          borderRadius: BorderRadius.circular(AppSizes.radiusCard),
        ),
        child: chord.keys.isEmpty
            ? Text(
                context.tr('함께 누를 키를 골라 주세요'),
                style: TextStyle(color: AppColors.textMuted),
              )
            : ExcludeSemantics(
                child: Wrap(
                  alignment: WrapAlignment.center,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  spacing: AppSizes.p4,
                  runSpacing: AppSizes.p8,
                  children: [
                    for (final (index, label) in keyLabels.indexed) ...[
                      if (index > 0)
                        Text('+', style: TextStyle(color: AppColors.textMuted)),
                      _Keycap(label),
                    ],
                  ],
                ),
              ),
      ),
    );
  }
}

class _Keycap extends StatelessWidget {
  const _Keycap(this.label);

  final String label;

  @override
  Widget build(BuildContext context) => Container(
    padding: EdgeInsets.symmetric(
      horizontal: AppSizes.p12,
      vertical: AppSizes.p8,
    ),
    decoration: BoxDecoration(
      color: AppColors.buttonNeutral,
      borderRadius: BorderRadius.circular(AppSizes.radiusLg),
      border: Border(
        bottom: BorderSide(color: AppColors.borderStrong, width: 3),
      ),
    ),
    child: Text(
      label,
      style: monoStyle(
        color: AppColors.textPrimary,
        fontSize: 16,
        fontWeight: FontWeight.w600,
      ),
    ),
  );
}

class _SpecialKeyPicker extends StatelessWidget {
  const _SpecialKeyPicker({
    required this.ctrl,
    required this.alt,
    required this.shift,
    required this.selectedKeys,
    required this.onCtrl,
    required this.onAlt,
    required this.onShift,
    required this.onSpecialKey,
  });

  final bool ctrl;
  final bool alt;
  final bool shift;
  final List<String> selectedKeys;
  final ValueChanged<bool> onCtrl;
  final ValueChanged<bool> onAlt;
  final ValueChanged<bool> onShift;
  final ValueChanged<String> onSpecialKey;

  @override
  Widget build(BuildContext context) => Wrap(
    spacing: AppSizes.p8,
    runSpacing: AppSizes.p8,
    children: [
      _keyChip('combo-modifier-ctrl', 'Ctrl', ctrl, onCtrl),
      _keyChip('combo-modifier-alt', 'Alt', alt, onAlt),
      _keyChip('combo-modifier-shift', 'Shift', shift, onShift),
      for (final key in terminalComboKeys())
        _keyChip(
          'combo-special-${key.id}',
          key.label,
          selectedKeys.contains(key.id),
          (_) => onSpecialKey(key.id),
        ),
    ],
  );

  Widget _keyChip(
    String id,
    String label,
    bool selected,
    ValueChanged<bool> onSelected,
  ) => Semantics(
    identifier: id,
    child: FilterChip(
      key: ValueKey(id),
      label: Text(label),
      selected: selected,
      showCheckmark: false,
      selectedColor: AppColors.brandSubtle,
      backgroundColor: AppColors.buttonNeutral,
      side: BorderSide(
        color: selected ? AppColors.brandPrimary : Colors.transparent,
      ),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(AppSizes.radiusLg),
      ),
      labelStyle: monoStyle(
        color: AppColors.textPrimary,
        fontSize: 14,
        fontWeight: selected ? FontWeight.w600 : null,
      ),
      onSelected: onSelected,
    ),
  );
}
