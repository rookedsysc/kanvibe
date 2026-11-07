import '../../task_create/domain/session_type.dart';

class TerminalShortcut {
  const TerminalShortcut(
    this.id,
    this.label,
    this.description, {
    this.input,
    this.action,
    this.chord,
  });

  final String id;
  final String label;
  final String description;
  final String? input;
  final String? action;
  final TerminalShortcutChord? chord;

  TerminalShortcut.combo(this.id, this.label, TerminalShortcutChord chord)
    : description = chord.label,
      input = null,
      action = null,
      chord = chord;

  /// 사용자가 만든 콤보인지. 아니면 [terminalShortcuts]가 주는 기본 세팅이다
  bool get isCombo => chord != null;
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
const maxTerminalComboLabelLength = 12;

/// 한 콤보에 함께 누를 수 있는 키 수. 보조 키(Ctrl·Alt·Shift)는 세지 않는다
const maxTerminalChordKeys = 8;

/// 콤보 편집기의 특수 키 목록. Enter와 Backspace는 입력란으로 받을 수 없어 여기서 고른다
List<TerminalShortcut> terminalComboKeys() => [
  ..._commonShortcuts.where(
    (key) => key.input == null && key.id != 'ctrl' && key.id != 'alt',
  ),
  const TerminalShortcut('enter', 'Enter', 'Enter'),
  const TerminalShortcut('backspace', 'Backspace', 'Backspace'),
  const TerminalShortcut('insert', 'Insert', 'Insert'),
  const TerminalShortcut('delete', 'Delete', 'Delete'),
  for (var number = 1; number <= 12; number++)
    TerminalShortcut('f$number', 'F$number', 'F$number'),
];

/// 보조 키를 누른 채 [keys]를 함께 누르는 조합.
///
/// 터미널은 "보조 키 + 키 하나"를 한 단위로 받으므로, 여러 키를 함께 누르면 각 키가 같은 보조 키와 묶여
/// 차례로 전달된다. 실제 키보드에서 Ctrl을 누른 채 a와 b를 누를 때와 같은 결과다.
/// [keys]의 각 항목은 한 글자면 입력 문자이고, 그보다 길면 [terminalComboKeys]의 특수 키 id다.
class TerminalShortcutChord {
  const TerminalShortcutChord({
    required this.keys,
    this.ctrl = false,
    this.alt = false,
    this.shift = false,
  });

  final List<String> keys;
  final bool ctrl;
  final bool alt;
  final bool shift;

  static const _shiftedCharacters = {
    '1': '!',
    '2': '@',
    '3': '#',
    '4': r'$',
    '5': '%',
    '6': '^',
    '7': '&',
    '8': '*',
    '9': '(',
    '0': ')',
    '-': '_',
    '=': '+',
    '[': '{',
    ']': '}',
    r'\': '|',
    ';': ':',
    "'": '"',
    ',': '<',
    '.': '>',
    '/': '?',
    '`': '~',
  };

  static bool isCharacterKey(String key) => key.runes.length == 1;

  /// Shift를 함께 눌렀을 때 [character] 대신 입력되는 글자
  String shiftedCharacter(String character) {
    if (!shift) return character;
    if (_shiftedCharacters[character] case final shifted?) return shifted;
    final rune = character.runes.single;
    if (rune >= 0x61 && rune <= 0x7a) return String.fromCharCode(rune - 0x20);
    return character;
  }

  bool get isValid =>
      keys.isNotEmpty &&
      keys.length <= maxTerminalChordKeys &&
      keys.toSet().length == keys.length &&
      keys.every(_isValidKey);

  bool _isValidKey(String key) {
    if (!isCharacterKey(key)) {
      return terminalComboKeys().any((candidate) => candidate.id == key);
    }
    final rune = shiftedCharacter(key).runes.single;
    if (rune < 0x20 || rune == 0x7f) return false;
    return !ctrl ||
        (rune >= 0x40 && rune <= 0x7e) ||
        rune == 0x20 ||
        rune == 0x3f;
  }

  String get label => [
    if (ctrl) 'Ctrl',
    if (alt) 'Alt',
    if (shift) 'Shift',
    for (final key in keys) keyLabel(key),
  ].join('+');

  static String keyLabel(String key) {
    if (key == ' ') return 'Space';
    if (isCharacterKey(key)) return key;
    return terminalComboKeys()
            .where((candidate) => candidate.id == key)
            .firstOrNull
            ?.label ??
        key;
  }

  /// 같은 키를 같은 보조 키로 누르는 조합인지. 다른 세션에서 콤보를 가져올 때 중복을 거른다
  bool isSameChord(TerminalShortcutChord other) =>
      ctrl == other.ctrl &&
      alt == other.alt &&
      shift == other.shift &&
      keys.length == other.keys.length &&
      Iterable.generate(keys.length).every((i) => keys[i] == other.keys[i]);
}

/// 세션 종류 하나의 단축키 바 구성.
///
/// 바에는 [terminalShortcuts]가 주는 기본 세팅과 사용자가 만든 [combos]가 함께 들어간다.
/// [ids]는 바에 보이는 순서이고, [removedDefaultIds]는 설정에서 지운 기본 세팅이다.
class TerminalShortcutProfile {
  TerminalShortcutProfile({
    this.enabled = true,
    required List<String> ids,
    List<TerminalShortcut> combos = const [],
    List<String> removedDefaultIds = const [],
  }) : removedDefaultIds = List.unmodifiable(removedDefaultIds.toSet()),
       ids = List.unmodifiable(
         ids
             .where(
               (id) =>
                   !removedDefaultIds.contains(id) &&
                   (!_isComboId(id) || combos.any((combo) => combo.id == id)),
             )
             .toSet(),
       ),
       combos = List.unmodifiable(combos);

  final bool enabled;
  final List<String> ids;
  final List<TerminalShortcut> combos;
  final List<String> removedDefaultIds;

  factory TerminalShortcutProfile.defaults() =>
      TerminalShortcutProfile(ids: []);

  static bool _isComboId(String id) => id.startsWith('combo-');

  /// [type] 세션에서 남아 있는 기본 세팅. 지운 기본 세팅은 빠진다
  List<TerminalShortcut> remainingDefaults(SessionType type) => [
    for (final shortcut in terminalShortcuts(type))
      if (!removedDefaultIds.contains(shortcut.id)) shortcut,
  ];

  /// [ids] 순서대로 바에 보일 단축키. [type] 세션에 없는 기본 세팅 id는 건너뛴다
  List<TerminalShortcut> visibleShortcuts(SessionType type) {
    final shortcutsById = {
      for (final shortcut in [...remainingDefaults(type), ...combos])
        shortcut.id: shortcut,
    };
    return [for (final id in ids) ?shortcutsById[id]];
  }

  /// [source] 설정을 이 설정에 합친다.
  ///
  /// 같은 키 조합이 이미 있는 콤보는 건너뛰고, 나머지는 [newComboIdPrefix]로 시작하는 새 id로 복사해
  /// 최대 개수까지 바에 붙인다. [source]가 바에 띄운 기본 세팅은 [type] 세션에도 있으면 지웠더라도 되살려 띄운다.
  TerminalShortcutProfile mergedWith(
    TerminalShortcutProfile source,
    SessionType type, {
    required String newComboIdPrefix,
  }) {
    final mergedCombos = [...combos];
    final mergedIds = [...ids];
    final mergedRemovedIds = {...removedDefaultIds};
    final targetDefaultIds = terminalShortcuts(type).map((key) => key.id);
    for (final id in source.ids) {
      if (_isComboId(id) || !targetDefaultIds.contains(id)) continue;
      mergedRemovedIds.remove(id);
      if (!mergedIds.contains(id)) mergedIds.add(id);
    }
    for (final (index, combo) in source.combos.indexed) {
      if (mergedCombos.length >= maxTerminalCombos) break;
      final isDuplicate = mergedCombos.any(
        (existing) => existing.chord!.isSameChord(combo.chord!),
      );
      if (isDuplicate) continue;
      final copied = TerminalShortcut.combo(
        '$newComboIdPrefix-$index',
        combo.label,
        combo.chord!,
      );
      mergedCombos.add(copied);
      mergedIds.add(copied.id);
    }
    return TerminalShortcutProfile(
      enabled: enabled,
      ids: mergedIds,
      combos: mergedCombos,
      removedDefaultIds: mergedRemovedIds.toList(),
    );
  }

  /// 이 설정을 [source]와 같은 구성으로 바꾼다. 표시 여부만 이 설정의 값을 지킨다.
  ///
  /// 콤보는 [newComboIdPrefix]로 시작하는 새 id로 복사한다. [type] 세션에 없는 기본 세팅은 건너뛰고,
  /// [type] 세션에만 있는 기본 세팅은 지우지 않은 채 바에서만 뺀다.
  TerminalShortcutProfile overwrittenBy(
    TerminalShortcutProfile source,
    SessionType type, {
    required String newComboIdPrefix,
  }) {
    final copiedCombos = {
      for (final (index, combo) in source.combos.indexed)
        combo.id: TerminalShortcut.combo(
          '$newComboIdPrefix-$index',
          combo.label,
          combo.chord!,
        ),
    };
    final targetDefaultIds = terminalShortcuts(type).map((key) => key.id);
    return TerminalShortcutProfile(
      enabled: enabled,
      combos: copiedCombos.values.toList(),
      ids: [
        for (final id in source.ids)
          if (copiedCombos[id] case final copied?)
            copied.id
          else if (targetDefaultIds.contains(id))
            id,
      ],
      removedDefaultIds: source.removedDefaultIds
          .where(targetDefaultIds.contains)
          .toList(),
    );
  }
}
