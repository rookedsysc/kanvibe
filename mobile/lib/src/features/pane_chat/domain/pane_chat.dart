import '../../chat_history/domain/chat_history.dart';

typedef PaneChatTarget = ({String taskId, String paneId});

/// 대화를 어디까지 받았는지. epoch이 다르면 seq는 견줄 수 없다
typedef PaneChatCursor = ({String epoch, int seq});

/// 데스크탑이 밀어 준 대화 줄 하나
typedef PaneChatRow = ({int seq, ChatMessage message});

/// 밀려온 줄을 가진 대화에 반영한 결과
enum PaneChatRowsOutcome {
  /// 새 줄이 붙었거나 있던 줄이 자랐다
  applied,

  /// 이미 가진 내용이라 달라진 것이 없다
  alreadyApplied,

  /// 중간 줄을 놓쳤거나 순번의 기준이 달라 전체를 다시 받아야 한다
  outOfSync,
}

enum PaneChatInputState { ready, occupied, unknown }

enum PaneChatKey { up, down, left, right, enter, escape }

enum PaneChatAction {
  prompt('prompt'),
  interrupt('interrupt'),
  modelPicker('model-picker'),
  modelSetting('model-setting'),
  key('key');

  const PaneChatAction(this.path);
  final String path;
}

/// effort 슬라이더의 한 칸
class PaneChatEffortLevel {
  const PaneChatEffortLevel({required this.id, required this.label});
  final String id, label;
}

class PaneChatModelOption {
  const PaneChatModelOption({
    required this.id,
    required this.label,
    this.effortLevels = const [],
    this.supportsFast = false,
  });
  final String id, label;

  /// 낮은 단계부터. 비어 있으면 이 모델은 effort를 고를 수 없다
  final List<PaneChatEffortLevel> effortLevels;
  final bool supportsFast;
}

/// CLI의 현재 모델 설정과 고를 수 있는 선택지
class PaneChatModelSettings {
  const PaneChatModelSettings({
    required this.models,
    this.model,
    this.effort,
    this.isFast,
  });
  final String? model, effort;

  /// 기록에서 알아내지 못했으면 null이다
  final bool? isFast;
  final List<PaneChatModelOption> models;

  PaneChatModelOption? get currentModel =>
      models.where((option) => option.id == model).firstOrNull;

  static PaneChatModelSettings? fromJson(Object? value) {
    if (value is! Map<String, dynamic>) return null;
    return PaneChatModelSettings(
      model: value['model'] as String?,
      effort: value['effort'] as String?,
      isFast: value['isFast'] as bool?,
      models: [
        for (final model in value['models'] as List<dynamic>? ?? const [])
          if (model is Map<String, dynamic>)
            PaneChatModelOption(
              id: model['id'] as String,
              label: model['label'] as String? ?? model['id'] as String,
              supportsFast: model['supportsFast'] == true,
              effortLevels: [
                for (final level
                    in model['effortLevels'] as List<dynamic>? ?? const [])
                  if (level is Map<String, dynamic>)
                    PaneChatEffortLevel(
                      id: level['id'] as String,
                      label: level['label'] as String? ?? level['id'] as String,
                    ),
              ],
            ),
      ],
    );
  }
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
    this.composer = const PaneChatCapabilities(),
    this.modelSettings,
  });

  /// 현재 모델·effort와 선택지. 오래된 데스크탑이거나 기록에서 읽지 못했으면 null이다
  final PaneChatModelSettings? modelSettings;

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
  final PaneChatCapabilities composer;

  /// 데스크탑이 밀어 준 상태에서 실려 온 값만 덮어쓴다. 대화 줄은 건드리지 않는다
  PaneChatSnapshot applyPushedState(Map<String, dynamic> pushed) {
    T pick<T>(String key, T current, T Function(Object? value) read) =>
        pushed.containsKey(key) ? read(pushed[key]) : current;
    final modelPicker = pushed['modelPicker'] as Map<String, dynamic>?;
    final pushedTranscript = pushed['transcript'] as Map<String, dynamic>?;
    return PaneChatSnapshot(
      taskId: taskId,
      paneId: paneId,
      sessionType: pick(
        'sessionType',
        sessionType,
        (value) => value as String?,
      ),
      provider: pick('provider', provider, (value) => value as String?),
      targetToken: pick(
        'targetToken',
        targetToken,
        (value) => value as String?,
      ),
      canSend: pick('canSend', canSend, (value) => value == true),
      inputState: pick(
        'inputState',
        inputState,
        (value) =>
            PaneChatInputState.values
                .where((state) => state.name == value)
                .firstOrNull ??
            PaneChatInputState.unknown,
      ),
      screen: pick('screen', screen, (value) => value as String? ?? ''),
      reason: pick('reason', reason, (value) => value as String?),
      transcript: pushedTranscript == null
          ? transcript
          : transcript.applyPushedStatus(pushedTranscript),
      modelPickerSupported: modelPicker == null
          ? modelPickerSupported
          : modelPicker['supported'] == true &&
                modelPicker['mode'] == 'native-cli',
      composer: pick(
        'composer',
        composer,
        (value) =>
            PaneChatCapabilities.fromJson(value as Map<String, dynamic>? ?? {}),
      ),
      modelSettings: pick(
        'modelSettings',
        modelSettings,
        PaneChatModelSettings.fromJson,
      ),
    );
  }

  PaneChatSnapshot withTranscript(PaneChatTranscript next) => PaneChatSnapshot(
    taskId: taskId,
    paneId: paneId,
    sessionType: sessionType,
    provider: provider,
    targetToken: targetToken,
    canSend: canSend,
    inputState: inputState,
    screen: screen,
    reason: reason,
    transcript: next,
    modelPickerSupported: modelPickerSupported,
    composer: composer,
    modelSettings: modelSettings,
  );

  factory PaneChatSnapshot.fromJson(Map<String, dynamic> value) {
    final modelPicker = value['modelPicker'] as Map<String, dynamic>?;
    return PaneChatSnapshot(
      composer: PaneChatCapabilities.fromJson(
        value['composer'] as Map<String, dynamic>? ?? {},
      ),
      modelSettings: PaneChatModelSettings.fromJson(value['modelSettings']),
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
    this.epoch,
    this.seq = 0,
  });

  /// 화면에 쌓아 두는 최근 대화의 최대 줄 수. 데스크탑의 조회 창과 같다
  static const _maxMessages = 100;

  /// 대화에 매긴 순번이 유효한 범위. 순번을 매기지 않는 데스크탑에서는 null이다
  final String? epoch;

  /// 가장 최근 메시지의 순번. [messages]는 최신순이라 index 0이 이 순번이다
  final int seq;

  PaneChatCursor? get cursor {
    final current = epoch;
    return current == null ? null : (epoch: current, seq: seq);
  }

  /// 밀려온 줄을 순번에 맞춰 반영한다. 다음 순번이면 붙이고, 이미 가진 순번이면 그 줄을 바꾼다
  ({PaneChatTranscript transcript, PaneChatRowsOutcome outcome}) applyRows(
    String rowsEpoch,
    List<PaneChatRow> rows,
  ) {
    if (epoch == null || epoch != rowsEpoch) {
      return (transcript: this, outcome: PaneChatRowsOutcome.outOfSync);
    }
    final ordered = [...rows]..sort((a, b) => a.seq.compareTo(b.seq));
    final next = [...messages];
    var endSeq = seq;
    var changed = false;
    for (final row in ordered) {
      if (row.seq > endSeq + 1) {
        return (transcript: this, outcome: PaneChatRowsOutcome.outOfSync);
      }
      if (row.seq == endSeq + 1) {
        next.insert(0, row.message);
        endSeq = row.seq;
        changed = true;
        continue;
      }
      final index = endSeq - row.seq;
      if (index < next.length) {
        next[index] = row.message;
        changed = true;
      }
    }
    if (!changed) {
      return (transcript: this, outcome: PaneChatRowsOutcome.alreadyApplied);
    }
    return (
      transcript: PaneChatTranscript(
        available: available,
        reason: reason,
        sessionId: sessionId,
        messages: next.length > _maxMessages
            ? next.sublist(0, _maxMessages)
            : next,
        nextCursor: nextCursor,
        epoch: epoch,
        seq: endSeq,
      ),
      outcome: PaneChatRowsOutcome.applied,
    );
  }

  PaneChatTranscript applyPushedStatus(Map<String, dynamic> pushed) =>
      PaneChatTranscript(
        available: pushed['status'] == 'available',
        reason: pushed['reason'] as String?,
        sessionId: pushed['sessionId'] as String?,
        messages: messages,
        nextCursor: pushed['nextCursor'] as String?,
        epoch: epoch,
        seq: seq,
      );

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
        epoch: value['epoch'] as String?,
        seq: value['seq'] as int? ?? 0,
      );
}

class PaneChatCapabilities {
  const PaneChatCapabilities({
    this.attachments = false,
    this.skills = false,
    this.fileMentions = false,
    this.maxAttachmentBytes = 10 * 1024 * 1024,
    this.maxAttachments = 10,
  });
  final bool attachments, skills, fileMentions;
  final int maxAttachmentBytes, maxAttachments;
  factory PaneChatCapabilities.fromJson(Map<String, dynamic> value) =>
      PaneChatCapabilities(
        attachments: value['attachments'] == true,
        skills: value['skills'] == true,
        fileMentions: value['fileMentions'] == true,
        maxAttachmentBytes:
            value['maxAttachmentBytes'] as int? ?? 10 * 1024 * 1024,
        maxAttachments: value['maxAttachments'] as int? ?? 10,
      );
}

class PaneChatSuggestion {
  const PaneChatSuggestion({
    required this.id,
    required this.label,
    required this.description,
    required this.insertText,
  });
  final String id, label, description, insertText;
  factory PaneChatSuggestion.fromJson(Map<String, dynamic> value) =>
      PaneChatSuggestion(
        id: value['id'] as String,
        label: value['label'] as String,
        description: value['description'] as String? ?? '',
        insertText: value['insertText'] as String,
      );
}
