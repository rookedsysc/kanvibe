import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_command.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../../chat_history/domain/chat_history.dart';
import '../domain/pane_chat.dart';
import 'pane_chat_subscriptions.dart';

final paneChatProvider = AsyncNotifierProvider.autoDispose
    .family<PaneChatController, PaneChatState, PaneChatTarget>(
      PaneChatController.new,
      retry: (_, _) => null,
    );

/// 보냈지만 아직 CLI 대화 기록에 나타나지 않은 메시지. 화면은 기록을 기다리지 않고 이것부터 그린다
final paneChatPendingPromptProvider = NotifierProvider.autoDispose
    .family<PaneChatPendingPrompt, String?, PaneChatTarget>(
      PaneChatPendingPrompt.new,
    );

class PaneChatPendingPrompt extends Notifier<String?> {
  PaneChatPendingPrompt(this.target);
  final PaneChatTarget target;

  /// `/clear` 같은 명령은 기록에 남지 않으므로, 기록에서 찾지 못해도 이 시간이 지나면 거둔다
  static const _recordWaitLimit = Duration(seconds: 20);
  Timer? _expiry;
  int _recordedCountAtSend = 0;

  @override
  String? build() {
    ref.onDispose(() => _expiry?.cancel());
    return null;
  }

  void show(String text, PaneChatTranscript transcript) {
    _expiry?.cancel();
    _recordedCountAtSend = _countRecorded(text, transcript);
    _expiry = Timer(_recordWaitLimit, clear);
    state = text;
  }

  void clear() {
    _expiry?.cancel();
    if (ref.mounted) state = null;
  }

  /// 같은 문장을 다시 보낸 경우에도 이전 기록과 헷갈리지 않도록, 보낼 때보다 기록이 늘었는지로 판단한다
  void retireIfRecorded(PaneChatTranscript transcript) {
    final text = state;
    if (text != null &&
        _countRecorded(text, transcript) > _recordedCountAtSend) {
      clear();
    }
  }

  int _countRecorded(String text, PaneChatTranscript transcript) {
    final sent = text.trim();
    return transcript.messages
        .where(
          (message) => message.role == 'user' && message.text.trim() == sent,
        )
        .length;
  }
}

class PaneChatState {
  const PaneChatState({
    required this.snapshot,
    required this.connectionId,
    this.isRefreshing = false,
    this.isActing = false,
    this.isStale = false,
    this.readError,
    this.actionError,
    this.canRetryAction = false,
  });

  final PaneChatSnapshot snapshot;
  final String connectionId;
  final bool isRefreshing;
  final bool isActing;
  final bool isStale;
  final String? readError;
  final String? actionError;
  final bool canRetryAction;

  bool get canControl =>
      !isActing && !isStale && snapshot.canSend && snapshot.targetToken != null;
  bool get canPrompt =>
      canControl && snapshot.inputState == PaneChatInputState.ready;
}

typedef _PendingAction = ({
  PaneChatAction action,
  String token,
  String? text,
  PaneChatKey? key,
});

class PaneChatController extends AsyncNotifier<PaneChatState> {
  PaneChatController(this.target);
  final PaneChatTarget target;

  static const _pollInterval = Duration(seconds: 2);

  /// 응답이 오가는 동안에는 더 자주 확인해 새 답변이 늦게 보이지 않게 한다
  static const _livePollInterval = Duration(milliseconds: 700);
  static const _livePollsAfterPrompt = 20;
  static const _livePollsAfterChange = 5;
  int _livePollsRemaining = 0;
  Timer? _pollTimer;
  DesktopConnection? _connection;
  Object? _request;
  Object? _pendingAction;
  Completer<void>? _readCompletion;
  int _generation = 0;
  int _activityGeneration = 0;
  bool _active = true;
  _PendingAction? _lastAction;

  /// 데스크탑이 변화를 밀어 주고 있는지. 그동안에는 주기적으로 조회하지 않는다
  bool _isLive = false;
  bool _wantsScreen = false;

  /// 조회 중에 push가 다시 받으라고 알렸는지. 진행 중인 조회가 끝나면 한 번 더 조회한다
  bool _isCatchUpPending = false;
  PaneChatSubscriber? _subscriber;

  @override
  Future<PaneChatState> build() async {
    final generation = ++_generation;
    _pollTimer?.cancel();
    _request = null;
    _pendingAction = null;
    _readCompletion = null;
    _lastAction = null;
    final subscriptions = ref.read(paneChatSubscriptionsProvider);
    ref.onDispose(() {
      _generation++;
      _pollTimer?.cancel();
      final subscriber = _subscriber;
      if (subscriber != null) subscriptions.unsubscribe(subscriber);
      final read = _readCompletion;
      if (read != null && !read.isCompleted) read.complete();
    });
    final connection = (await ref.watch(connectionControllerProvider.future))
        .activeConnection;
    if (connection == null) throw const DesktopDisconnectedException();
    _connection = connection;
    final snapshot = await _fetch(connection);
    if (ref.mounted && generation == _generation) {
      _schedulePoll();
      _subscribe();
    }
    return PaneChatState(snapshot: snapshot, connectionId: connection.id);
  }

  void _subscribe() {
    if (!_active || !ref.mounted) return;
    final subscriber = PaneChatSubscriber(
      target: target,
      includesChat: true,
      wantsScreen: _wantsScreen,
      readCursor: () => state.value?.snapshot.transcript.cursor,
      onEvent: _receivePush,
      onLiveChanged: _changeLive,
    );
    _subscriber = subscriber;
    ref.read(paneChatSubscriptionsProvider).subscribe(subscriber);
  }

  void _unsubscribe() {
    final subscriber = _subscriber;
    _subscriber = null;
    _isLive = false;
    if (subscriber != null) {
      ref.read(paneChatSubscriptionsProvider).unsubscribe(subscriber);
    }
  }

  /// CLI 화면이 보이는 동안에만 화면 변화를 받는다
  void setWantsScreen(bool wantsScreen) {
    if (_wantsScreen == wantsScreen) return;
    _wantsScreen = wantsScreen;
    if (_subscriber != null) _subscribe();
  }

  void _changeLive({required bool isLive, required bool isBehind}) {
    if (!ref.mounted || !_active) return;
    _isLive = isLive;
    if (!isLive) {
      _schedulePoll();
      return;
    }
    _pollTimer?.cancel();
    if (isBehind) _catchUp();
  }

  /// push만으로는 이어 갈 수 없을 때 스냅샷을 다시 받는다
  void _catchUp() {
    if (_request != null) {
      _isCatchUpPending = true;
      return;
    }
    unawaited(refresh());
  }

  void _runPendingCatchUp() {
    if (!_isCatchUpPending) return;
    _isCatchUpPending = false;
    _catchUp();
  }

  void _receivePush(Map<String, dynamic> event) {
    final previous = state.value;
    if (!ref.mounted || !_active || previous == null) return;
    switch (event['type']) {
      case 'pane-chat-replaced':
        _catchUp();
      case 'pane-chat-rows':
        final applied = previous.snapshot.transcript.applyRows(
          event['epoch']?.toString() ?? '',
          [
            for (final row in event['rows'] as List<dynamic>? ?? const [])
              (
                seq: (row as Map<String, dynamic>)['seq'] as int,
                message: ChatMessage.fromJson(
                  row['message'] as Map<String, dynamic>,
                ),
              ),
          ],
        );
        if (applied.outcome == PaneChatRowsOutcome.outOfSync) {
          _catchUp();
        } else if (applied.outcome == PaneChatRowsOutcome.applied) {
          ref
              .read(paneChatPendingPromptProvider(target).notifier)
              .retireIfRecorded(applied.transcript);
          _replaceSnapshot(
            previous,
            previous.snapshot.withTranscript(applied.transcript),
          );
        }
      case 'pane-chat-state':
        final pushed = event['state'] as Map<String, dynamic>? ?? const {};
        final snapshot = previous.snapshot.applyPushedState(pushed);
        final before = previous.snapshot.transcript;
        final after = snapshot.transcript;
        if (snapshot.targetToken != previous.snapshot.targetToken) {
          // 다른 CLI로 바뀌었으면 가진 대화도 그 CLI의 것이 아니다
          _lastAction = null;
          ref.read(paneChatPendingPromptProvider(target).notifier).clear();
          _replaceSnapshot(previous, snapshot, keepsActionError: false);
          _catchUp();
          return;
        }
        _replaceSnapshot(previous, snapshot);
        if (after.available &&
            (!before.available || after.sessionId != before.sessionId)) {
          _catchUp();
        }
    }
  }

  void _replaceSnapshot(
    PaneChatState previous,
    PaneChatSnapshot snapshot, {
    bool keepsActionError = true,
  }) {
    state = AsyncData(
      PaneChatState(
        snapshot: snapshot,
        connectionId: previous.connectionId,
        isRefreshing: previous.isRefreshing,
        isActing: previous.isActing,
        isStale: previous.isStale,
        readError: previous.readError,
        actionError: keepsActionError ? previous.actionError : null,
        canRetryAction: keepsActionError && previous.canRetryAction,
      ),
    );
  }

  Future<PaneChatSnapshot> _fetch(DesktopConnection connection) async {
    try {
      return await ref
          .read(desktopClientProvider)
          .fetchPaneChat(connection, target);
    } on DesktopRequestException catch (error) {
      await _forgetUnauthorized(connection, error);
      rethrow;
    }
  }

  Future<void> _forgetUnauthorized(
    DesktopConnection connection,
    DesktopRequestException error,
  ) async {
    if (error.isUnauthorized && ref.mounted) {
      await ref
          .read(connectionControllerProvider.notifier)
          .forgetLocally(connection.id, connection.token);
    }
  }

  void _schedulePoll() {
    _pollTimer?.cancel();
    if (!ref.mounted || !_active || _isLive) return;
    final isAwaitingReply = _livePollsRemaining > 0;
    if (isAwaitingReply) _livePollsRemaining--;
    _pollTimer = Timer(
      isAwaitingReply ? _livePollInterval : _pollInterval,
      () => unawaited(refresh()),
    );
  }

  void setActive(bool active) {
    if (_active == active) return;
    _active = active;
    _activityGeneration++;
    _pollTimer?.cancel();
    if (!active) {
      _unsubscribe();
      return;
    }
    _subscribe();
    final previous = state.value;
    if (previous != null) {
      state = AsyncData(
        PaneChatState(
          snapshot: previous.snapshot,
          connectionId: previous.connectionId,
          isActing: previous.isActing,
          isRefreshing: previous.isRefreshing,
          isStale: true,
          actionError: previous.actionError,
        ),
      );
      unawaited(refresh());
    }
  }

  Future<void> refresh() async {
    final previous = state.value;
    final connection = _connection;
    if (previous == null ||
        connection == null ||
        _request != null ||
        !_active) {
      return;
    }
    final generation = _generation;
    final activityGeneration = _activityGeneration;
    final request = Object();
    final completion = Completer<void>();
    _readCompletion = completion;
    _request = request;
    _pollTimer?.cancel();
    state = AsyncData(
      PaneChatState(
        snapshot: previous.snapshot,
        connectionId: previous.connectionId,
        isRefreshing: true,
        isActing: _pendingAction != null,
        isStale: previous.isStale,
        readError: previous.readError,
        actionError: previous.actionError,
        canRetryAction: previous.canRetryAction,
      ),
    );
    try {
      var snapshot = await _fetch(connection);
      if (!ref.mounted ||
          generation != _generation ||
          activityGeneration != _activityGeneration) {
        return;
      }
      // 조회가 오가는 사이에 push로 먼저 받은 줄을 더 오래된 응답으로 덮지 않는다
      final pushedTranscript = state.value?.snapshot.transcript;
      if (pushedTranscript != null &&
          pushedTranscript.epoch != null &&
          pushedTranscript.epoch == snapshot.transcript.epoch &&
          pushedTranscript.seq > snapshot.transcript.seq) {
        snapshot = snapshot.withTranscript(pushedTranscript);
      }
      final sameTarget = snapshot.targetToken == previous.snapshot.targetToken;
      if (!sameTarget) _lastAction = null;
      final pendingPrompt = ref.read(
        paneChatPendingPromptProvider(target).notifier,
      );
      if (sameTarget) {
        pendingPrompt.retireIfRecorded(snapshot.transcript);
      } else {
        pendingPrompt.clear();
      }
      if (_hasNewTranscript(
            previous.snapshot.transcript,
            snapshot.transcript,
          ) &&
          _livePollsRemaining < _livePollsAfterChange) {
        _livePollsRemaining = _livePollsAfterChange;
      }
      state = AsyncData(
        PaneChatState(
          snapshot: snapshot,
          connectionId: connection.id,
          isActing: _pendingAction != null,
          actionError: sameTarget ? previous.actionError : null,
          canRetryAction:
              sameTarget &&
              _lastAction != null &&
              _lastAction!.action != PaneChatAction.prompt,
        ),
      );
    } catch (_) {
      if (!ref.mounted ||
          generation != _generation ||
          activityGeneration != _activityGeneration) {
        return;
      }
      state = AsyncData(
        PaneChatState(
          snapshot: previous.snapshot,
          connectionId: previous.connectionId,
          isStale: true,
          isActing: _pendingAction != null,
          readError: '현재 CLI 상태를 불러오지 못했습니다. 다시 확인해 주세요.',
          actionError: previous.actionError,
        ),
      );
    } finally {
      if (ref.mounted &&
          generation == _generation &&
          identical(_request, request)) {
        _request = null;
        if (_active && activityGeneration != _activityGeneration) {
          unawaited(refresh());
        } else {
          _schedulePoll();
          _runPendingCatchUp();
        }
      }
      if (identical(_readCompletion, completion)) _readCompletion = null;
      if (!completion.isCompleted) completion.complete();
    }
  }

  bool _hasNewTranscript(PaneChatTranscript before, PaneChatTranscript after) {
    if (before.messages.length != after.messages.length) return true;
    if (after.messages.isEmpty) return false;
    return before.messages.first.text != after.messages.first.text;
  }

  Future<bool> send(
    String text, {
    List<String>? attachmentIds,
    String? requestId,
  }) => _act(
    PaneChatAction.prompt,
    text: text,
    attachmentIds: attachmentIds,
    requestId: requestId,
  );
  Future<bool> interrupt() => _act(PaneChatAction.interrupt);
  Future<bool> openModelPicker() => _act(PaneChatAction.modelPicker);

  /// 모델·effort·빠른 모드 중 하나를 바꾼다. [change]는 데스크탑이 받는 변경 한 건의 모양이다
  Future<bool> changeModelSetting(Map<String, Object?> change) =>
      _act(PaneChatAction.modelSetting, modelSettingChange: change);
  Future<bool> sendKey(PaneChatKey key) => _act(PaneChatAction.key, key: key);

  Future<bool> retryAction() async {
    final last = _lastAction;
    if (last == null || state.value?.snapshot.targetToken != last.token) {
      return false;
    }
    return _act(last.action, text: last.text, key: last.key);
  }

  Future<bool> _act(
    PaneChatAction action, {
    String? text,
    PaneChatKey? key,
    List<String>? attachmentIds,
    String? requestId,
    Map<String, Object?>? modelSettingChange,
  }) async {
    final initial = state.value;
    final connection = _connection;
    if (!_active ||
        initial == null ||
        connection == null ||
        !initial.canControl ||
        _pendingAction != null) {
      return false;
    }
    if (_request != null && _readCompletion == null) return false;
    final token = initial.snapshot.targetToken;
    final generation = _generation;
    final activityGeneration = _activityGeneration;
    final intent = Object();
    _pendingAction = intent;
    _pollTimer?.cancel();
    state = AsyncData(
      PaneChatState(
        snapshot: initial.snapshot,
        connectionId: initial.connectionId,
        isRefreshing: initial.isRefreshing,
        isActing: true,
        isStale: initial.isStale,
        readError: initial.readError,
        actionError: initial.actionError,
      ),
    );
    try {
      final read = _readCompletion;
      if (read != null) await read.future;
      if (!ref.mounted ||
          generation != _generation ||
          !_active ||
          activityGeneration != _activityGeneration) {
        return false;
      }
      final current = state.value;
      if (current == null ||
          current.isStale ||
          current.snapshot.targetToken != token ||
          current.connectionId != connection.id) {
        if (current != null) {
          state = AsyncData(
            PaneChatState(
              snapshot: current.snapshot,
              connectionId: current.connectionId,
              isRefreshing: current.isRefreshing,
              isStale: current.isStale,
              readError: current.readError,
              actionError: 'CLI 상태가 변경되어 요청한 동작을 보내지 않았습니다. 현재 화면을 확인해 주세요.',
            ),
          );
        }
        return false;
      }
      return await _performAction(
        action,
        text: text,
        key: key,
        attachmentIds: attachmentIds,
        requestId: requestId,
        modelSettingChange: modelSettingChange,
      );
    } finally {
      if (ref.mounted &&
          generation == _generation &&
          identical(_pendingAction, intent)) {
        _pendingAction = null;
        final current = state.value;
        if (current != null) {
          state = AsyncData(
            PaneChatState(
              snapshot: current.snapshot,
              connectionId: current.connectionId,
              isRefreshing: current.isRefreshing,
              isStale: current.isStale,
              readError: current.readError,
              actionError: current.actionError,
              canRetryAction: current.canRetryAction,
            ),
          );
        }
        _schedulePoll();
      }
    }
  }

  Future<bool> _performAction(
    PaneChatAction action, {
    String? text,
    PaneChatKey? key,
    List<String>? attachmentIds,
    String? requestId,
    Map<String, Object?>? modelSettingChange,
  }) async {
    final previous = state.value;
    final connection = _connection;
    if (!_active ||
        previous == null ||
        connection == null ||
        previous.isStale ||
        !previous.snapshot.canSend ||
        previous.snapshot.targetToken == null ||
        _request != null) {
      return false;
    }
    if ((action == PaneChatAction.prompt ||
            action == PaneChatAction.modelPicker ||
            action == PaneChatAction.modelSetting) &&
        previous.snapshot.inputState != PaneChatInputState.ready) {
      return false;
    }
    if (action == PaneChatAction.prompt &&
        ((text == null || text.trim().isEmpty) &&
            (attachmentIds == null || attachmentIds.isEmpty))) {
      return false;
    }
    if (action == PaneChatAction.modelPicker &&
        !previous.snapshot.modelPickerSupported) {
      return false;
    }
    final token = previous.snapshot.targetToken!;
    final generation = _generation;
    final request = Object();
    _request = request;
    _pollTimer?.cancel();
    // 모델 설정은 바꿀 값을 다시 골라야 하므로 같은 요청을 되풀이하는 재시도 대상이 아니다
    _lastAction = action == PaneChatAction.modelSetting
        ? null
        : (action: action, token: token, text: text, key: key);
    state = AsyncData(
      PaneChatState(
        snapshot: previous.snapshot,
        connectionId: previous.connectionId,
        isActing: true,
      ),
    );
    final pendingPrompt = ref.read(
      paneChatPendingPromptProvider(target).notifier,
    );
    if (action == PaneChatAction.prompt && text != null && text.isNotEmpty) {
      pendingPrompt.show(text, previous.snapshot.transcript);
    }
    var accepted = false;
    try {
      await ref
          .read(desktopClientProvider)
          .sendPaneChatAction(
            connection,
            target.taskId,
            token,
            action,
            text: text,
            key: key,
            attachmentIds: attachmentIds,
            requestId: requestId,
            modelSettingChange: modelSettingChange,
          );
      if (!ref.mounted || generation != _generation) return false;
      accepted = true;
      _lastAction = null;
      if (action == PaneChatAction.prompt) {
        _livePollsRemaining = _livePollsAfterPrompt;
      }
      state = AsyncData(
        PaneChatState(
          snapshot: previous.snapshot,
          connectionId: previous.connectionId,
        ),
      );
    } catch (error) {
      if (error is DesktopRequestException) {
        await _forgetUnauthorized(connection, error);
      }
      if (!ref.mounted || generation != _generation) return false;
      if (action == PaneChatAction.prompt) pendingPrompt.clear();
      final targetChanged =
          error is DesktopRequestException && error.statusCode == 409;
      state = AsyncData(
        PaneChatState(
          snapshot: previous.snapshot,
          connectionId: previous.connectionId,
          isStale: targetChanged,
          actionError: error is DesktopRequestException && error.message != null
              ? error.message
              : 'CLI 동작을 완료하지 못했습니다. 입력 내용을 확인하고 다시 시도해 주세요.',
          canRetryAction:
              !targetChanged &&
              action != PaneChatAction.prompt &&
              action != PaneChatAction.modelSetting,
        ),
      );
    } finally {
      if (ref.mounted &&
          generation == _generation &&
          identical(_request, request)) {
        _request = null;
        _schedulePoll();
        // 전송이 받아들여졌으면 바로 아래에서 다시 조회하므로 따로 따라잡지 않는다
        if (accepted) {
          _isCatchUpPending = false;
        } else {
          _runPendingCatchUp();
        }
      }
    }
    if (accepted) await refresh();
    return accepted;
  }
}
