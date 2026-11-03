import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter/material.dart';
import 'package:xterm/xterm.dart' as xterm;

import '../../../task_create/domain/session_type.dart';
import '../../../../common/constants/app_colors.dart';
import '../../domain/terminal_shortcut.dart';
import 'terminal_shortcut_bar.dart';

/// 소프트 키보드로 친 글자를 터미널에 넣는다.
///
/// xterm이 내장한 입력기를 쓰지 않는 이유는 글자가 겹쳐 들어가기 때문이다. 그 입력기는 IME가 알려 주는
/// 버퍼 전체를 새 입력으로 읽은 뒤 IME에 "버퍼를 비워라"를 보낸다. Gboard처럼 그 요청을 반영하기 전에
/// 다음 글자를 보내는 IME에서는 이미 보낸 앞 글자가 다시 실려 와서, `touch`가 `tttouch`로 찍힌다.
///
/// 여기서는 IME에 바뀐 부분만 알려 달라고(delta 모델) 요청한다. 무엇을 보낼지는 IME가 알려 준 변화만으로
/// 정하므로, 버퍼를 언제 비우든 이미 보낸 글자를 다시 보낼 일이 없다.
///
/// 터미널을 누른 것은 xterm의 탭 콜백이 아니라 포인터 이벤트로 알아챈다. xterm 4.0.0의 `onTapUp`은
/// 제스처 처리기가 받아 두기만 하고 부르지 않는다.
class TerminalSoftKeyboard extends StatefulWidget {
  const TerminalSoftKeyboard({
    required this.terminal,
    required this.controller,
    required this.child,
    this.sessionType = SessionType.terminal,
    this.onSessionAction,
    this.isBusy = false,
    super.key,
  });

  final xterm.Terminal terminal;
  final xterm.TerminalController controller;
  final Widget child;
  final SessionType sessionType;
  final ValueChanged<String>? onSessionAction;
  final bool isBusy;

  @override
  State<TerminalSoftKeyboard> createState() => _TerminalSoftKeyboardState();
}

class _TerminalSoftKeyboardState extends State<TerminalSoftKeyboard>
    with TextInputClient
    implements DeltaTextInputClient {
  bool _ctrlPressed = false;
  bool _altPressed = false;
  bool _isShortcutBarVisible = true;
  TextInputConnection? _connection;
  TextEditingValue _buffer = _emptyBuffer;

  final _activePointers = <int>{};
  int? _tapPointer;
  Offset? _pointerDownPosition;
  Duration? _pointerDownTime;

  /// 빈 버퍼를 공백 두 칸으로 둔다. 지울 글자가 없으면 IME가 백스페이스를 알리지 않고 삼키기 때문이다
  static const _emptyBuffer = TextEditingValue(
    text: '  ',
    selection: TextSelection.collapsed(offset: 2),
  );

  /// IME 버퍼가 이만큼 쌓이면 비운다. 비우지 않으면 한 세션 동안 친 글자가 전부 IME에 남는다
  static const _bufferResetLength = 256;
  static const _panelHeightFraction = .5;

  @override
  void dispose() {
    _closeKeyboard();
    super.dispose();
  }

  @override
  void didUpdateWidget(TerminalSoftKeyboard oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.sessionType != widget.sessionType) {
      _ctrlPressed = false;
      _altPressed = false;
    }
  }

  void _pressShortcut(TerminalShortcut shortcut) {
    if (shortcut.chord case final chord?) {
      _clearModifiers();
      if (!chord.isValid) return;
      if (chord.key == 'character') {
        final rune = chord.characterWithShift.runes.single;
        final text = chord.ctrl
            ? String.fromCharCode(rune == 0x3f ? 0x7f : rune & 0x1f)
            : chord.characterWithShift;
        widget.terminal.textInput('${chord.alt ? '\x1b' : ''}$text');
      } else {
        _sendTerminalKey(
          chord.key,
          ctrl: chord.ctrl,
          alt: chord.alt,
          shift: chord.shift,
        );
      }
      return;
    }
    if (shortcut.id == 'ctrl' || shortcut.id == 'alt') {
      setState(() {
        if (shortcut.id == 'ctrl') {
          _ctrlPressed = !_ctrlPressed;
        } else {
          _altPressed = !_altPressed;
        }
      });
      return;
    }
    if (shortcut.action case final action?) {
      _clearModifiers();
      widget.onSessionAction?.call(action);
      return;
    }
    if (shortcut.input case final input?) {
      widget.terminal.textInput(input);
    } else {
      _sendTerminalKey(shortcut.id, ctrl: _ctrlPressed, alt: _altPressed);
    }
    _clearModifiers();
  }

  void _sendTerminalKey(
    String id, {
    bool ctrl = false,
    bool alt = false,
    bool shift = false,
  }) {
    final cursorCode = switch (id) {
      'up' => 'A',
      'down' => 'B',
      'right' => 'C',
      'left' => 'D',
      'home' => 'H',
      'end' => 'F',
      _ => null,
    };
    final modifier = 1 + (shift ? 1 : 0) + (alt ? 2 : 0) + (ctrl ? 4 : 0);
    if (cursorCode != null) {
      final prefix = widget.terminal.cursorKeysMode ? '\x1bO' : '\x1b[';
      widget.terminal.textInput(
        modifier == 1 ? '$prefix$cursorCode' : '\x1b[1;$modifier$cursorCode',
      );
      return;
    }
    final tildeCode = switch (id) {
      'insert' => 2,
      'delete' => 3,
      'pageUp' => 5,
      'pageDown' => 6,
      _ => null,
    };
    if (tildeCode != null) {
      widget.terminal.textInput(
        modifier == 1 ? '\x1b[$tildeCode~' : '\x1b[$tildeCode;$modifier~',
      );
      return;
    }
    final key = switch (id) {
      'escape' => xterm.TerminalKey.escape,
      'enter' => xterm.TerminalKey.enter,
      'backspace' => xterm.TerminalKey.backspace,
      'tab' => xterm.TerminalKey.tab,
      'f1' => xterm.TerminalKey.f1,
      'f2' => xterm.TerminalKey.f2,
      'f3' => xterm.TerminalKey.f3,
      'f4' => xterm.TerminalKey.f4,
      'f5' => xterm.TerminalKey.f5,
      'f6' => xterm.TerminalKey.f6,
      'f7' => xterm.TerminalKey.f7,
      'f8' => xterm.TerminalKey.f8,
      'f9' => xterm.TerminalKey.f9,
      'f10' => xterm.TerminalKey.f10,
      'f11' => xterm.TerminalKey.f11,
      'f12' => xterm.TerminalKey.f12,
      _ => null,
    };
    if (key != null) {
      widget.terminal.keyInput(key, ctrl: ctrl, alt: alt, shift: shift);
    }
  }

  void _collapseShortcutBar() => setState(() {
    _isShortcutBarVisible = false;
    _ctrlPressed = false;
    _altPressed = false;
  });

  void _clearModifiers() {
    if (!_ctrlPressed && !_altPressed) return;
    setState(() {
      _ctrlPressed = false;
      _altPressed = false;
    });
  }

  /// 제스처 경쟁에 끼지 않는 [Listener]로 본다. 탭 인식기를 하나 더 두면 xterm의 선택·스크롤 제스처와 겨루게 된다.
  ///
  /// 포커스는 xterm이 쥐고 있으므로 따로 만들지 않고 바깥에서 지켜보기만 한다. 상태 시트처럼 다른 화면이
  /// 위에 떠서 터미널이 포커스를 잃으면 키보드를 내린다. 남겨 두면 새로 뜬 화면의 아래쪽을 덮는다.
  @override
  Widget build(BuildContext context) => Focus(
    canRequestFocus: false,
    skipTraversal: true,
    onFocusChange: (hasFocus) {
      if (!hasFocus) {
        _discardTap();
        _closeKeyboard();
        _clearModifiers();
      }
    },
    child: LayoutBuilder(
      builder: (context, constraints) => Column(
        children: [
          Expanded(
            child: Stack(
              fit: StackFit.expand,
              children: [
                Listener(
                  onPointerDown: _onPointerDown,
                  onPointerMove: _onPointerMove,
                  onPointerUp: _onPointerUp,
                  onPointerCancel: (event) {
                    _activePointers.remove(event.pointer);
                    _discardTap();
                  },
                  child: widget.child,
                ),
                if (!_isShortcutBarVisible)
                  Positioned(
                    right: 8,
                    bottom: 8,
                    child: Focus(
                      canRequestFocus: false,
                      descendantsAreFocusable: false,
                      child: Semantics(
                        identifier: 'shortcut-expand',
                        child: IconButton.filledTonal(
                          tooltip: '단축키 바 펼치기',
                          style: IconButton.styleFrom(
                            backgroundColor: AppColors.buttonNeutral,
                            foregroundColor: AppColors.textPrimary,
                          ),
                          onPressed: () =>
                              setState(() => _isShortcutBarVisible = true),
                          icon: const Icon(Icons.keyboard_arrow_up),
                        ),
                      ),
                    ),
                  ),
              ],
            ),
          ),
          if (_isShortcutBarVisible)
            TerminalShortcutBar(
              sessionType: widget.sessionType,
              onShortcut: _pressShortcut,
              onCollapse: _collapseShortcutBar,
              isBusy: widget.isBusy,
              maxPanelHeight: constraints.maxHeight * _panelHeightFraction,
            ),
        ],
      ),
    ),
  );

  void _onPointerDown(PointerDownEvent event) {
    _activePointers.add(event.pointer);
    _discardTap();
    // xterm은 tapDown에서 선택을 지우므로 손가락을 댈 때 이미 선택 중이었는지 먼저 확인한다.
    if (_activePointers.length != 1 || widget.controller.selection != null) {
      return;
    }
    _tapPointer = event.pointer;
    _pointerDownPosition = event.position;
    _pointerDownTime = event.timeStamp;
  }

  void _onPointerMove(PointerMoveEvent event) {
    final start = _pointerDownPosition;
    if (event.pointer == _tapPointer &&
        start != null &&
        (event.position - start).distance > kTouchSlop) {
      _discardTap();
    }
  }

  void _onPointerUp(PointerUpEvent event) {
    _activePointers.remove(event.pointer);
    final startedAt = _pointerDownTime;
    final start = _pointerDownPosition;
    final isShortTap =
        event.pointer == _tapPointer &&
        startedAt != null &&
        start != null &&
        (event.position - start).distance <= kTouchSlop &&
        event.timeStamp - startedAt < kLongPressTimeout &&
        _activePointers.isEmpty &&
        widget.controller.selection == null;
    _discardTap();
    if (isShortTap) _requestKeyboard();
  }

  void _discardTap() {
    _tapPointer = null;
    _pointerDownPosition = null;
    _pointerDownTime = null;
  }

  /// 터미널을 눌렀을 때 키보드를 띄운다. 이미 붙어 있으면 내려간 키보드만 다시 올린다
  void _requestKeyboard() {
    final connection = _connection;
    if (connection != null && connection.attached) {
      connection.show();
      return;
    }

    _connection = TextInput.attach(
      this,
      const TextInputConfiguration(
        /// 자동 대문자와 자동 수정이 없는 키보드. 명령어의 첫 글자가 대문자로 바뀌면 안 된다
        inputType: TextInputType.emailAddress,
        inputAction: TextInputAction.done,
        autocorrect: false,
        enableSuggestions: false,
        enableIMEPersonalizedLearning: false,
        enableDeltaModel: true,
      ),
    );
    _resetBuffer();
    _connection!.show();
  }

  void _closeKeyboard() {
    _connection?.close();
    _connection = null;
  }

  void _resetBuffer() {
    _buffer = _emptyBuffer;
    _connection?.setEditingState(_emptyBuffer);
  }

  @override
  void updateEditingValueWithDeltas(List<TextEditingDelta> deltas) {
    for (final delta in deltas) {
      _sendToTerminal(delta);

      /// 우리가 들고 있던 버퍼가 아니라 IME가 기준으로 삼은 버퍼에 적용한다.
      /// 비우라는 요청을 IME가 아직 반영하지 않았으면 둘이 다르기 때문이다
      _buffer = delta.apply(TextEditingValue(text: delta.oldText));
    }

    if (_buffer.text.length < _emptyBuffer.text.length ||
        _buffer.text.length > _bufferResetLength) {
      _resetBuffer();
    }
  }

  void _sendToTerminal(TextEditingDelta delta) {
    switch (delta) {
      case TextEditingDeltaInsertion(:final textInserted):
        _typeText(textInserted);
      case TextEditingDeltaDeletion(:final deletedRange):
        _pressBackspace(deletedRange.end - deletedRange.start);
      case TextEditingDeltaReplacement(
        :final replacedRange,
        :final replacementText,
      ):
        _pressBackspace(replacedRange.end - replacedRange.start);
        _typeText(replacementText);
      case TextEditingDeltaNonTextUpdate():
        break;
    }
  }

  /// 줄바꿈은 글자가 아니라 Enter 키로 보낸다. 셸은 Enter를 받아야 명령을 실행한다
  void _typeText(String text) {
    final lines = text.split('\n');
    for (var index = 0; index < lines.length; index++) {
      if (index > 0) {
        _pressModifiedKey(xterm.TerminalKey.enter);
      }
      if (lines[index].isNotEmpty) {
        _typeModifiedText(lines[index]);
      }
    }
  }

  void _typeModifiedText(String text) {
    if (!_ctrlPressed && !_altPressed) {
      widget.terminal.textInput(text);
      return;
    }
    final runes = text.runes.toList();
    final first = runes.first;
    final canControl =
        (first >= 0x40 && first <= 0x7f) || first == 0x20 || first == 0x3f;
    if (_ctrlPressed && !canControl) {
      widget.terminal.textInput(text);
      return;
    }
    final character = _ctrlPressed
        ? String.fromCharCode(first == 0x3f ? 0x7f : first & 0x1f)
        : String.fromCharCode(first);
    widget.terminal.textInput('${_altPressed ? '\x1b' : ''}$character');
    _clearModifiers();
    if (runes.length > 1) {
      widget.terminal.textInput(String.fromCharCodes(runes.skip(1)));
    }
  }

  void _pressModifiedKey(xterm.TerminalKey key) {
    widget.terminal.keyInput(key, ctrl: _ctrlPressed, alt: _altPressed);
    _clearModifiers();
  }

  void _pressBackspace(int count) {
    for (var index = 0; index < count; index++) {
      _pressModifiedKey(xterm.TerminalKey.backspace);
    }
  }

  @override
  void performAction(TextInputAction action) {
    if (action == TextInputAction.done || action == TextInputAction.newline) {
      _pressModifiedKey(xterm.TerminalKey.enter);
    }
  }

  @override
  TextEditingValue? get currentTextEditingValue => _buffer;

  @override
  AutofillScope? get currentAutofillScope => null;

  /// delta 모델을 요청했으므로 버퍼 전체는 오지 않는다
  @override
  void updateEditingValue(TextEditingValue value) {}

  @override
  void connectionClosed() {
    _connection = null;
    _clearModifiers();
  }

  @override
  void updateFloatingCursor(RawFloatingCursorPoint point) {}

  @override
  void showAutocorrectionPromptRect(int start, int end) {}

  @override
  void performPrivateCommand(String action, Map<String, dynamic> data) {}
}
