import '../../task_create/domain/session_type.dart';

class TerminalShortcut {
  const TerminalShortcut(
    this.id,
    this.label,
    this.description, {
    this.input,
    this.action,
    this.strokes = const [],
  });

  final String id;
  final String label;
  final String description;
  final String? input;
  final String? action;
  final List<TerminalShortcutStroke> strokes;

  TerminalShortcut.combo(
    this.id,
    this.label,
    List<TerminalShortcutStroke> strokes,
  ) : description = strokes.map((stroke) => stroke.label).join(' → '),
      input = null,
      action = null,
      strokes = List.unmodifiable(strokes);
}

const _commonShortcuts = [
  TerminalShortcut('escape', 'Esc', 'Escape'),
  TerminalShortcut('tab', 'Tab', '자동 완성 · Tab'),
  TerminalShortcut('ctrl', 'Ctrl', '다음 키에 Ctrl 적용'),
  TerminalShortcut('alt', 'Alt', '다음 키에 Alt 적용'),
  TerminalShortcut('left', '←', '왼쪽 화살표'),
  TerminalShortcut('down', '↓', '아래쪽 화살표'),
  TerminalShortcut('up', '↑', '위쪽 화살표'),
  TerminalShortcut('right', '→', '오른쪽 화살표'),
  TerminalShortcut('interrupt', '^C', '실행 중단 · Ctrl+C', input: '\x03'),
  TerminalShortcut('eof', '^D', '입력 종료 · Ctrl+D', input: '\x04'),
  TerminalShortcut('suspend', '^Z', '프로세스 일시 정지 · Ctrl+Z', input: '\x1a'),
  TerminalShortcut('clear', '^L', '화면 다시 그리기 · Ctrl+L', input: '\x0c'),
  TerminalShortcut('home', 'Home', '줄 처음'),
  TerminalShortcut('end', 'End', '줄 끝'),
  TerminalShortcut('pageUp', 'PgUp', 'Page Up'),
  TerminalShortcut('pageDown', 'PgDn', 'Page Down'),
];

const _multiplexerShortcuts = [
  TerminalShortcut(
    'split-right',
    '좌우 분할',
    'pane을 좌우로 분할',
    action: 'split-right',
  ),
  TerminalShortcut('split-down', '상하 분할', 'pane을 상하로 분할', action: 'split-down'),
  TerminalShortcut(
    'toggle-fullscreen',
    '확대',
    'pane 확대 전환',
    action: 'toggle-fullscreen',
  ),
  TerminalShortcut('scroll-up', '이전 출력', '세션 출력 위로 스크롤', action: 'scroll-up'),
  TerminalShortcut(
    'scroll-down',
    '다음 출력',
    '세션 출력 아래로 스크롤',
    action: 'scroll-down',
  ),
  TerminalShortcut(
    'scroll-bottom',
    '맨 아래',
    '세션 스크롤 종료',
    action: 'scroll-bottom',
  ),
];

List<TerminalShortcut> terminalShortcuts(SessionType type) => [
  ..._commonShortcuts,
  if (type != SessionType.terminal) ..._multiplexerShortcuts,
  if (type == SessionType.tmux)
    const TerminalShortcut(
      'copy-mode',
      '복사 모드',
      'tmux 복사 모드',
      action: 'copy-mode',
    ),
  if (type == SessionType.zellij)
    const TerminalShortcut(
      'toggle-floating',
      '플로팅',
      'Zellij 플로팅 pane 전환',
      action: 'toggle-floating',
    ),
];

const maxTerminalCombos = 12;
const maxTerminalComboStrokes = 8;
const maxTerminalComboLabelLength = 12;

List<TerminalShortcut> terminalComboKeys() => [
  ..._commonShortcuts.where(
    (key) => key.input == null && key.id != 'ctrl' && key.id != 'alt',
  ),
  const TerminalShortcut('enter', 'Enter', 'Enter'),
  const TerminalShortcut('backspace', 'Backspace', 'Backspace'),
];

class TerminalShortcutStroke {
  const TerminalShortcutStroke({
    required this.key,
    this.character = '',
    this.ctrl = false,
    this.alt = false,
  });

  final String key;
  final String character;
  final bool ctrl;
  final bool alt;

  bool get isValid {
    if (key != 'character') {
      return terminalComboKeys().any((candidate) => candidate.id == key);
    }
    if (character.runes.length != 1) return false;
    final rune = character.runes.single;
    if (rune < 0x20 || rune == 0x7f) return false;
    return !ctrl ||
        (rune >= 0x40 && rune <= 0x7e) ||
        rune == 0x20 ||
        rune == 0x3f;
  }

  String get label {
    final keyLabel = key == 'character'
        ? (character == ' ' ? 'Space' : character)
        : terminalComboKeys()
                  .where((candidate) => candidate.id == key)
                  .firstOrNull
                  ?.label ??
              key;
    return [if (ctrl) 'Ctrl', if (alt) 'Alt', keyLabel].join('+');
  }

  TerminalShortcutStroke copyWith({
    String? key,
    String? character,
    bool? ctrl,
    bool? alt,
  }) => TerminalShortcutStroke(
    key: key ?? this.key,
    character: character ?? this.character,
    ctrl: ctrl ?? this.ctrl,
    alt: alt ?? this.alt,
  );
}

class TerminalShortcutProfile {
  TerminalShortcutProfile({
    this.enabled = true,
    required List<String> ids,
    List<TerminalShortcut> combos = const [],
  }) : ids = List.unmodifiable(ids),
       combos = List.unmodifiable(combos);

  final bool enabled;
  final List<String> ids;
  final List<TerminalShortcut> combos;

  List<TerminalShortcut> shortcuts(SessionType type) => [
    ...terminalShortcuts(type),
    ...combos,
  ];

  factory TerminalShortcutProfile.defaults(SessionType type) =>
      TerminalShortcutProfile(
        ids: terminalShortcuts(type).map((key) => key.id).toList(),
      );
}
