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
  late final _strokes = [
    for (final stroke
        in widget.combo?.strokes ??
            [
              const TerminalShortcutStroke(
                key: 'character',
                character: 'c',
                ctrl: true,
              ),
            ])
      (id: _nextId++, stroke: stroke),
  ];
  int _nextId = 0;
  String? _error;
  int _selectedIndex = 0;

  @override
  void dispose() {
    _label.dispose();
    super.dispose();
  }

  void _change(int index, TerminalShortcutStroke stroke) => setState(() {
    _strokes[index] = (id: _strokes[index].id, stroke: stroke);
    _error = null;
  });

  void _submit() {
    final label = _label.text.trim();
    if (label.isEmpty ||
        label.length > maxTerminalComboLabelLength ||
        _strokes.isEmpty ||
        _strokes.any((entry) => !entry.stroke.isValid)) {
      setState(() => _error = '이름과 각 단계의 키를 확인해 주세요. Ctrl 문자는 영문·기호를 사용하세요.');
      return;
    }
    Navigator.pop(
      context,
      TerminalShortcut.combo(
        widget.combo?.id ?? 'combo-${DateTime.now().microsecondsSinceEpoch}',
        label,
        _strokes.map((entry) => entry.stroke).toList(),
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
              padding: const EdgeInsets.symmetric(horizontal: 20),
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
                        hintText: '예: 다시 실행',
                      ),
                      onChanged: (_) => setState(() => _error = null),
                    ),
                  ),
                  const SizedBox(height: AppSizes.p16),
                  Text(
                    '키 입력 순서',
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  const SizedBox(height: AppSizes.p8),
                  Container(
                    padding: const EdgeInsets.all(AppSizes.p12),
                    decoration: BoxDecoration(
                      color: AppColors.terminalBg,
                      borderRadius: BorderRadius.circular(AppSizes.radiusLg),
                      border: Border.all(color: AppColors.borderDefault),
                    ),
                    child: Wrap(
                      spacing: AppSizes.p8,
                      runSpacing: AppSizes.p8,
                      children: [
                        for (var index = 0; index < _strokes.length; index++)
                          Semantics(
                            identifier: 'combo-step-${_strokes[index].id}',
                            child: InputChip(
                              label: Text(
                                '${index + 1}  ${_strokes[index].stroke.label}',
                                style: monoStyle(color: AppColors.textPrimary),
                              ),
                              selected: index == _selectedIndex,
                              selectedColor: AppColors.brandSubtle,
                              deleteIconColor: AppColors.textSecondary,
                              checkmarkColor: AppColors.textPrimary,
                              side: const BorderSide(
                                color: AppColors.borderStrong,
                              ),
                              onPressed: () =>
                                  setState(() => _selectedIndex = index),
                              deleteButtonTooltipMessage: '${index + 1}번째 키 삭제',
                              onDeleted: _strokes.length == 1
                                  ? null
                                  : () => setState(() {
                                      _strokes.removeAt(index);
                                      if (index < _selectedIndex) {
                                        _selectedIndex--;
                                      }
                                      _selectedIndex = _selectedIndex.clamp(
                                        0,
                                        _strokes.length - 1,
                                      );
                                      _error = null;
                                    }),
                            ),
                          ),
                      ],
                    ),
                  ),
                  const Padding(
                    padding: EdgeInsets.symmetric(vertical: AppSizes.p12),
                    child: Text(
                      '키를 눌러 편집하세요. 왼쪽부터 순서대로 전송합니다.',
                      style: TextStyle(color: AppColors.textSecondary),
                    ),
                  ),
                  _strokeEditor(_selectedIndex),
                  Row(
                    children: [
                      TextButton.icon(
                        onPressed: _selectedIndex == 0
                            ? null
                            : () => _moveStep(-1),
                        icon: const Icon(Icons.arrow_back, size: 16),
                        label: const Text('앞으로'),
                      ),
                      TextButton.icon(
                        onPressed: _selectedIndex == _strokes.length - 1
                            ? null
                            : () => _moveStep(1),
                        icon: const Icon(Icons.arrow_forward, size: 16),
                        label: const Text('뒤로'),
                      ),
                    ],
                  ),
                  TextButton.icon(
                    onPressed: _strokes.length >= maxTerminalComboStrokes
                        ? null
                        : () => setState(() {
                            _strokes.add((
                              id: _nextId++,
                              stroke: const TerminalShortcutStroke(
                                key: 'enter',
                              ),
                            ));
                            _selectedIndex = _strokes.length - 1;
                            _error = null;
                          }),
                    icon: const Icon(Icons.add),
                    label: Text(
                      '키 단계 추가 (${_strokes.length}/$maxTerminalComboStrokes)',
                    ),
                  ),
                ],
              ),
            ),
          ),
          if (_error != null)
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 8),
              child: Text(
                _error!,
                style: const TextStyle(color: AppColors.statusError),
              ),
            ),
          SafeArea(
            top: false,
            child: Padding(
              padding: const EdgeInsets.all(16),
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

  void _moveStep(int offset) => setState(() {
    final destination = _selectedIndex + offset;
    _strokes.insert(destination, _strokes.removeAt(_selectedIndex));
    _selectedIndex = destination;
    _error = null;
  });

  Widget _strokeEditor(int index) {
    final entry = _strokes[index];
    final stroke = entry.stroke;
    return Padding(
      key: ValueKey(entry.id),
      padding: const EdgeInsets.only(bottom: 12),
      child: Column(
        children: [
          Row(
            children: [
              Expanded(
                child: Semantics(
                  identifier: 'combo-key-${entry.id}',
                  child: DropdownButtonFormField<String>(
                    key: ValueKey('${entry.id}-${stroke.key}'),
                    initialValue: stroke.key,
                    isExpanded: true,
                    decoration: InputDecoration(labelText: '${index + 1}번째 키'),
                    items: [
                      const DropdownMenuItem(
                        value: 'character',
                        child: Text('문자'),
                      ),
                      for (final key in terminalComboKeys())
                        DropdownMenuItem(value: key.id, child: Text(key.label)),
                    ],
                    onChanged: (key) {
                      if (key != null) {
                        _change(index, stroke.copyWith(key: key));
                      }
                    },
                  ),
                ),
              ),
              if (stroke.key == 'character') ...[
                const SizedBox(width: 8),
                SizedBox(
                  width: 80,
                  child: Semantics(
                    identifier: 'combo-character-${entry.id}',
                    child: TextFormField(
                      key: ValueKey('${entry.id}-${stroke.key}'),
                      initialValue: stroke.character,
                      inputFormatters: [LengthLimitingTextInputFormatter(1)],
                      autocorrect: false,
                      enableSuggestions: false,
                      decoration: const InputDecoration(labelText: '문자'),
                      onChanged: (character) =>
                          _change(index, stroke.copyWith(character: character)),
                    ),
                  ),
                ),
              ],
            ],
          ),
          Wrap(
            spacing: 8,
            runSpacing: 4,
            children: [
              FilterChip(
                label: const Text('Ctrl'),
                selectedColor: AppColors.brandSubtle,
                checkmarkColor: AppColors.textPrimary,
                side: const BorderSide(color: AppColors.borderStrong),
                labelStyle: const TextStyle(color: AppColors.textPrimary),
                selected: stroke.ctrl,
                onSelected: (value) =>
                    _change(index, stroke.copyWith(ctrl: value)),
              ),
              FilterChip(
                label: const Text('Alt'),
                selectedColor: AppColors.brandSubtle,
                checkmarkColor: AppColors.textPrimary,
                side: const BorderSide(color: AppColors.borderStrong),
                labelStyle: const TextStyle(color: AppColors.textPrimary),
                selected: stroke.alt,
                onSelected: (value) =>
                    _change(index, stroke.copyWith(alt: value)),
              ),
            ],
          ),
        ],
      ),
    );
  }
}
