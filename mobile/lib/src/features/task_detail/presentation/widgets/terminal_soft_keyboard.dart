import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:xterm/xterm.dart' as xterm;

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
    required this.child,
    super.key,
  });

  final xterm.Terminal terminal;
  final Widget child;

  @override
  State<TerminalSoftKeyboard> createState() => _TerminalSoftKeyboardState();
}

class _TerminalSoftKeyboardState extends State<TerminalSoftKeyboard>
    with TextInputClient
    implements DeltaTextInputClient {
  TextInputConnection? _connection;
  TextEditingValue _buffer = _emptyBuffer;

  /// 손가락을 댄 자리. 뗀 자리가 여기서 크게 벗어나면 스크롤한 것이지 누른 것이 아니다
  Offset? _pointerDownPosition;

  /// 빈 버퍼를 공백 두 칸으로 둔다. 지울 글자가 없으면 IME가 백스페이스를 알리지 않고 삼키기 때문이다
  static const _emptyBuffer = TextEditingValue(
    text: '  ',
    selection: TextSelection.collapsed(offset: 2),
  );

  /// IME 버퍼가 이만큼 쌓이면 비운다. 비우지 않으면 한 세션 동안 친 글자가 전부 IME에 남는다
  static const _bufferResetLength = 256;

  @override
  void dispose() {
    _closeKeyboard();
    super.dispose();
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
        _closeKeyboard();
      }
    },
    child: Listener(
      onPointerDown: (event) => _pointerDownPosition = event.position,
      onPointerUp: (event) {
        final downPosition = _pointerDownPosition;
        _pointerDownPosition = null;
        if (downPosition != null &&
            (event.position - downPosition).distance <= kTouchSlop) {
          _requestKeyboard();
        }
      },
      child: widget.child,
    ),
  );

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
        widget.terminal.keyInput(xterm.TerminalKey.enter);
      }
      if (lines[index].isNotEmpty) {
        widget.terminal.textInput(lines[index]);
      }
    }
  }

  void _pressBackspace(int count) {
    for (var index = 0; index < count; index++) {
      widget.terminal.keyInput(xterm.TerminalKey.backspace);
    }
  }

  @override
  void performAction(TextInputAction action) {
    if (action == TextInputAction.done || action == TextInputAction.newline) {
      widget.terminal.keyInput(xterm.TerminalKey.enter);
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
  }

  @override
  void updateFloatingCursor(RawFloatingCursorPoint point) {}

  @override
  void showAutocorrectionPromptRect(int start, int end) {}

  @override
  void performPrivateCommand(String action, Map<String, dynamic> data) {}
}
