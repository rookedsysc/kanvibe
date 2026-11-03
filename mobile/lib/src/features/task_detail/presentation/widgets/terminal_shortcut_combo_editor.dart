import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_sizes.dart';
import '../../../../common/constants/app_theme.dart';
import '../../domain/terminal_shortcut.dart';

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
  late final _character = TextEditingController(
    text: widget.combo?.chord?.character ?? '',
  );
  late TerminalShortcutChord _chord =
      widget.combo?.chord ?? const TerminalShortcutChord(key: 'character');
  late bool _usesSpecialKey = terminalComboKeys().any(
    (key) => key.id == _chord.key,
  );
  String? _error;

  @override
  void dispose() {
    _label.dispose();
    _character.dispose();
    super.dispose();
  }

  void _change(TerminalShortcutChord chord) => setState(() {
    _chord = chord;
    _error = null;
  });

  void _submit() {
    final label = _label.text.trim();
    if (label.isEmpty ||
        label.length > maxTerminalComboLabelLength ||
        !_chord.isValid) {
      setState(() => _error = '이름과 조합할 키를 확인해 주세요. Ctrl 문자는 영문·기호를 사용하세요.');
      return;
    }
    Navigator.pop(
      context,
      TerminalShortcut.combo(
        widget.combo?.id ?? 'combo-${DateTime.now().microsecondsSinceEpoch}',
        label,
        _chord,
      ),
    );
  }

  @override
  Widget build(BuildContext context) => Padding(
    padding: EdgeInsets.only(bottom: MediaQuery.viewInsetsOf(context).bottom),
    child: SizedBox(
      height:
          (MediaQuery.sizeOf(context).height -
              MediaQuery.viewInsetsOf(context).bottom) *
          .9,
      child: Column(
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(20, 12, 8, 4),
            child: Row(
              children: [
                Expanded(
                  child: Text(
                    widget.combo == null ? '콤보 추가' : '콤보 편집',
                    style: Theme.of(context).textTheme.titleLarge,
                  ),
                ),
                IconButton(
                  tooltip: '콤보 닫기',
                  onPressed: () => Navigator.pop(context),
                  icon: const Icon(Icons.close),
                ),
              ],
            ),
          ),
          Expanded(
            child: SingleChildScrollView(
              padding: const EdgeInsets.symmetric(horizontal: AppSizes.p16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Semantics(
                    identifier: 'combo-name',
                    child: TextField(
                      controller: _label,
                      maxLength: maxTerminalComboLabelLength,
                      decoration: const InputDecoration(
                        labelText: '콤보 이름',
                        hintText: '예: 복사',
                      ),
                      onChanged: (_) => setState(() => _error = null),
                    ),
                  ),
                  const SizedBox(height: AppSizes.p16),
                  Text('키 조합', style: Theme.of(context).textTheme.titleMedium),
                  const SizedBox(height: AppSizes.p8),
                  Container(
                    padding: const EdgeInsets.all(AppSizes.p16),
                    decoration: BoxDecoration(
                      color: AppColors.terminalBg,
                      border: Border.all(color: AppColors.borderStrong),
                      borderRadius: BorderRadius.circular(AppSizes.radiusLg),
                    ),
                    child: Semantics(
                      identifier: 'combo-preview',
                      child: Text(
                        _chord.isValid ? _chord.label : '조합할 키를 입력하세요',
                        style: monoStyle(
                          color: AppColors.textPrimary,
                          fontSize: 18,
                        ),
                      ),
                    ),
                  ),
                  const Padding(
                    padding: EdgeInsets.symmetric(vertical: AppSizes.p12),
                    child: Text(
                      '보조 키와 입력 키를 동시에 누르는 조합입니다.',
                      style: TextStyle(color: AppColors.textSecondary),
                    ),
                  ),
                  Wrap(
                    spacing: AppSizes.p8,
                    runSpacing: AppSizes.p4,
                    children: [
                      _modifier(
                        'Ctrl',
                        _chord.ctrl,
                        (value) => _change(_chord.copyWith(ctrl: value)),
                      ),
                      _modifier(
                        'Alt',
                        _chord.alt,
                        (value) => _change(_chord.copyWith(alt: value)),
                      ),
                      _modifier(
                        'Shift',
                        _chord.shift,
                        (value) => _change(_chord.copyWith(shift: value)),
                      ),
                    ],
                  ),
                  const SizedBox(height: AppSizes.p16),
                  Wrap(
                    spacing: AppSizes.p8,
                    children: [
                      for (final special in [false, true])
                        ChoiceChip(
                          checkmarkColor: AppColors.textPrimary,
                          label: Text(special ? '특수 키 선택' : '키보드로 입력'),
                          selected: _usesSpecialKey == special,
                          selectedColor: AppColors.brandSubtle,
                          labelStyle: const TextStyle(
                            color: AppColors.textPrimary,
                          ),
                          onSelected: (_) => setState(() {
                            _usesSpecialKey = special;
                            _chord = _chord.copyWith(
                              key: special ? 'escape' : 'character',
                              character: special ? '' : _character.text,
                            );
                            _error = null;
                          }),
                        ),
                    ],
                  ),
                  const SizedBox(height: AppSizes.p12),
                  if (_usesSpecialKey)
                    Wrap(
                      spacing: AppSizes.p8,
                      runSpacing: AppSizes.p4,
                      children: [
                        for (final key in terminalComboKeys())
                          ChoiceChip(
                            checkmarkColor: AppColors.textPrimary,
                            key: ValueKey('combo-special-${key.id}'),
                            label: Text(key.label),
                            selected: _chord.key == key.id,
                            selectedColor: AppColors.brandSubtle,
                            labelStyle: const TextStyle(
                              color: AppColors.textPrimary,
                            ),
                            onSelected: (_) => _change(
                              _chord.copyWith(key: key.id, character: ''),
                            ),
                          ),
                      ],
                    )
                  else
                    Semantics(
                      identifier: 'combo-key-input',
                      child: TextField(
                        controller: _character,
                        autocorrect: false,
                        enableSuggestions: false,
                        textInputAction: TextInputAction.done,
                        inputFormatters: [LengthLimitingTextInputFormatter(1)],
                        decoration: const InputDecoration(
                          labelText: '입력 키',
                          hintText: '예: c, 1, /',
                        ),
                        onTap: () => _character.selection = TextSelection(
                          baseOffset: 0,
                          extentOffset: _character.text.length,
                        ),
                        onChanged: (text) => _change(
                          _chord.copyWith(
                            key: text.isEmpty ? 'backspace' : 'character',
                            character: text,
                          ),
                        ),
                        onSubmitted: (_) {
                          if (_character.text.isEmpty) {
                            _change(
                              _chord.copyWith(key: 'enter', character: ''),
                            );
                          }
                        },
                      ),
                    ),
                  const SizedBox(height: AppSizes.p16),
                ],
              ),
            ),
          ),
          if (_error != null)
            Padding(
              padding: const EdgeInsets.symmetric(
                horizontal: AppSizes.p16,
                vertical: AppSizes.p8,
              ),
              child: Text(
                _error!,
                style: const TextStyle(color: AppColors.statusError),
              ),
            ),
          SafeArea(
            top: false,
            child: Padding(
              padding: const EdgeInsets.all(AppSizes.p16),
              child: FilledButton(
                onPressed: _submit,
                child: const Text('콤보 저장'),
              ),
            ),
          ),
        ],
      ),
    ),
  );

  Widget _modifier(
    String label,
    bool selected,
    ValueChanged<bool> onSelected,
  ) => FilterChip(
    label: Text(label),
    selected: selected,
    selectedColor: AppColors.brandSubtle,
    checkmarkColor: AppColors.textPrimary,
    labelStyle: const TextStyle(color: AppColors.textPrimary),
    onSelected: onSelected,
  );
}
