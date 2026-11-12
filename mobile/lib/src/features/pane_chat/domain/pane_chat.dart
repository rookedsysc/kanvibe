import '../../chat_history/domain/chat_history.dart';

typedef PaneChatTarget = ({String taskId, String paneId});

enum PaneChatInputState { ready, occupied, unknown }

enum PaneChatKey { up, down, left, right, enter, escape }

enum PaneChatAction {
  prompt('prompt'),
  interrupt('interrupt'),
  modelPicker('model-picker'),
  key('key');

  const PaneChatAction(this.path);
  final String path;
}

class PaneChatSnapshot {
  const PaneChatSnapshot({
    required this.taskId,
    required this.paneId,
    required this.provider,
    required this.targetToken,
    required this.canSend,
    required this.inputState,
    required this.screen,
    required this.reason,
    required this.transcript,
    required this.modelPickerSupported,
    this.sessionType,
  });

  final String taskId;
  final String? paneId;
  final String? sessionType;
  final String? provider;
  final String? targetToken;
  final bool canSend;
  final PaneChatInputState inputState;
  final String screen;
  final String? reason;
  final PaneChatTranscript transcript;
  final bool modelPickerSupported;

  factory PaneChatSnapshot.fromJson(Map<String, dynamic> value) {
    final modelPicker = value['modelPicker'] as Map<String, dynamic>?;
    return PaneChatSnapshot(
      taskId: value['taskId'] as String,
      paneId: value['paneId'] as String?,
      sessionType: value['sessionType'] as String?,
      provider: value['provider'] as String?,
      targetToken: value['targetToken'] as String?,
      canSend: value['canSend'] == true,
      inputState:
          PaneChatInputState.values
              .where((state) => state.name == value['inputState'])
              .firstOrNull ??
          PaneChatInputState.unknown,
      screen: value['screen'] as String? ?? '',
      reason: value['reason'] as String?,
      transcript: PaneChatTranscript.fromJson(
        value['transcript'] as Map<String, dynamic>,
      ),
      modelPickerSupported:
          modelPicker?['supported'] == true &&
          modelPicker?['mode'] == 'native-cli',
    );
  }
}

class PaneChatTranscript {
  const PaneChatTranscript({
    required this.available,
    required this.messages,
    this.reason,
    this.sessionId,
    this.nextCursor,
  });

  final bool available;
  final String? reason;
  final String? sessionId;
  final List<ChatMessage> messages;
  final String? nextCursor;

  factory PaneChatTranscript.fromJson(Map<String, dynamic> value) =>
      PaneChatTranscript(
        available: value['status'] == 'available',
        reason: value['reason'] as String?,
        sessionId: value['sessionId'] as String?,
        messages: (value['messages'] as List<dynamic>? ?? [])
            .map(
              (message) =>
                  ChatMessage.fromJson(message as Map<String, dynamic>),
            )
            .toList(),
        nextCursor: value['nextCursor'] as String?,
      );
}
