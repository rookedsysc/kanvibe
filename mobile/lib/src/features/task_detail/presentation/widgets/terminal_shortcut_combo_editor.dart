import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../../../common/constants/app_colors.dart';
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
      height: MediaQuery.sizeOf(context).height * .72,
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
                  const SizedBox(height: 8),
                  Text(
                    _strokes.map((entry) => entry.stroke.label).join(' → '),
                    style: const TextStyle(color: AppColors.textSecondary),
                  ),
                  const Padding(
                    padding: EdgeInsets.symmetric(vertical: 12),
                    child: Text('각 단계의 키를 순서대로 보냅니다. Ctrl·Alt를 함께 선택할 수 있습니다.'),
                  ),
                  for (var index = 0; index < _strokes.length; index++)
                    _strokeEditor(index),
                  TextButton.icon(
                    onPressed: _strokes.length >= maxTerminalComboStrokes
                        ? null
                        : () => setState(
                            () => _strokes.add((
                              id: _nextId++,
                              stroke: const TerminalShortcutStroke(
                                key: 'enter',
                              ),
                            )),
                          ),
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
              IconButton(
                tooltip: '${index + 1}번째 키 삭제',
                onPressed: _strokes.length == 1
                    ? null
                    : () => setState(() => _strokes.removeAt(index)),
                icon: const Icon(Icons.remove_circle_outline),
              ),
            ],
          ),
          Wrap(
            spacing: 8,
            runSpacing: 4,
            children: [
              FilterChip(
                label: const Text('Ctrl'),
                selected: stroke.ctrl,
                onSelected: (value) =>
                    _change(index, stroke.copyWith(ctrl: value)),
              ),
              FilterChip(
                label: const Text('Alt'),
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
