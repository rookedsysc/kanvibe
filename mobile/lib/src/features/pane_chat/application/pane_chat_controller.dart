import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_command.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../domain/pane_chat.dart';

final paneChatProvider = AsyncNotifierProvider.autoDispose
    .family<PaneChatController, PaneChatState, PaneChatTarget>(
      PaneChatController.new,
      retry: (_, _) => null,
    );

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
  Timer? _pollTimer;
  DesktopConnection? _connection;
  Object? _request;
  Object? _pendingAction;
  Completer<void>? _readCompletion;
  int _generation = 0;
  int _activityGeneration = 0;
  bool _active = true;
  _PendingAction? _lastAction;

  @override
  Future<PaneChatState> build() async {
    final generation = ++_generation;
    _pollTimer?.cancel();
    _request = null;
    _pendingAction = null;
    _readCompletion = null;
    _lastAction = null;
    ref.onDispose(() {
      _generation++;
      _pollTimer?.cancel();
      final read = _readCompletion;
      if (read != null && !read.isCompleted) read.complete();
    });
    final connection = (await ref.watch(connectionControllerProvider.future))
        .activeConnection;
    if (connection == null) throw const DesktopDisconnectedException();
    _connection = connection;
    final snapshot = await _fetch(connection);
    if (ref.mounted && generation == _generation) _schedulePoll();
    return PaneChatState(snapshot: snapshot, connectionId: connection.id);
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
    if (!ref.mounted || !_active) return;
    _pollTimer = Timer(_pollInterval, () => unawaited(refresh()));
  }

  void setActive(bool active) {
    if (_active == active) return;
    _active = active;
    _activityGeneration++;
    _pollTimer?.cancel();
    if (!active) return;
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
      final snapshot = await _fetch(connection);
      if (!ref.mounted ||
          generation != _generation ||
          activityGeneration != _activityGeneration) {
        return;
      }
      final sameTarget = snapshot.targetToken == previous.snapshot.targetToken;
      if (!sameTarget) _lastAction = null;
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
        }
      }
      if (identical(_readCompletion, completion)) _readCompletion = null;
      if (!completion.isCompleted) completion.complete();
    }
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
            action == PaneChatAction.modelPicker) &&
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
    _lastAction = (action: action, token: token, text: text, key: key);
    state = AsyncData(
      PaneChatState(
        snapshot: previous.snapshot,
        connectionId: previous.connectionId,
        isActing: true,
      ),
    );
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
          );
      if (!ref.mounted || generation != _generation) return false;
      accepted = true;
      _lastAction = null;
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
          canRetryAction: !targetChanged && action != PaneChatAction.prompt,
        ),
      );
    } finally {
      if (ref.mounted &&
          generation == _generation &&
          identical(_request, request)) {
        _request = null;
        _schedulePoll();
      }
    }
    if (accepted) await refresh();
    return accepted;
  }
}
