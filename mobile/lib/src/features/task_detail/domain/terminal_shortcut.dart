import '../../task_create/domain/session_type.dart';

class TerminalShortcut {
  const TerminalShortcut(
    this.id,
    this.label,
    this.description, {
    this.input,
    this.action,
  });

  final String id;
  final String label;
  final String description;
  final String? input;
  final String? action;
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

class TerminalShortcutProfile {
  TerminalShortcutProfile({this.enabled = true, required List<String> ids})
    : ids = List.unmodifiable(ids);

  final bool enabled;
  final List<String> ids;

  factory TerminalShortcutProfile.defaults(SessionType type) =>
      TerminalShortcutProfile(
        ids: terminalShortcuts(type).map((key) => key.id).toList(),
      );
}
