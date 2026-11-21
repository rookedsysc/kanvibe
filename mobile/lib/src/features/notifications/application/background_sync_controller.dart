import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../data/background_sync_repository.dart';
import '../domain/background_sync_status.dart';
import 'mobile_notification_controller.dart';

final backgroundSyncRepositoryProvider = Provider<BackgroundSyncRepository>(
  (ref) => BackgroundSyncRepository(ref.watch(desktopClientProvider)),
);
final backgroundSyncProvider =
    AsyncNotifierProvider<BackgroundSyncController, BackgroundSyncState>(
      BackgroundSyncController.new,
      retry: (_, _) => null,
    );

class BackgroundSyncState {
  const BackgroundSyncState(
    this.status, {
    this.connection,
    this.isStarting = false,
    this.requestError,
  });

  final BackgroundSyncStatus status;
  final DesktopConnection? connection;
  final bool isStarting;
  final Object? requestError;

  bool belongsTo(DesktopConnection? active) =>
      connection?.id == active?.id && connection?.token == active?.token;
}

class BackgroundSyncController extends AsyncNotifier<BackgroundSyncState> {
  int _generation = 0;
  int _startSequence = 0;
  DesktopConnection? _connection;
  DesktopConnection? _startingConnection;

  bool _sameConnection(DesktopConnection? first, DesktopConnection? second) =>
      first != null &&
      second != null &&
      first.id == second.id &&
      first.token == second.token;

  bool _isCurrent(DesktopConnection connection) =>
      ref.mounted &&
      _sameConnection(
        ref.read(connectionControllerProvider).value?.activeConnection,
        connection,
      );

  @override
  Future<BackgroundSyncState> build() async {
    _generation++;
    _connection = null;
    final connections = ref.watch(connectionControllerProvider.future);
    final subscription = ref
        .watch(mobileNotificationControllerProvider)
        .syncUpdates
        .listen((connectionId) {
          if (ref.mounted && connectionId == _connection?.id) {
            ref.invalidateSelf();
          }
        });
    ref.onDispose(() => unawaited(subscription.cancel()));
    final connection = (await connections).activeConnection;
    _connection = connection;
    if (connection == null) {
      return const BackgroundSyncState(BackgroundSyncStatus.idle());
    }
    try {
      final status = await ref
          .read(backgroundSyncRepositoryProvider)
          .fetch(connection);
      return BackgroundSyncState(
        status,
        connection: connection,
        isStarting: _sameConnection(_startingConnection, connection),
      );
    } catch (error) {
      await _forgetUnauthorized(connection, error);
      rethrow;
    }
  }

  Future<void> _forgetUnauthorized(
    DesktopConnection connection,
    Object error,
  ) async {
    if (error is DesktopRequestException &&
        error.isUnauthorized &&
        _isCurrent(connection)) {
      await ref
          .read(connectionControllerProvider.notifier)
          .forgetLocally(connection.id, connection.token);
    }
  }

  Future<bool> start() async {
    final previous = state.value;
    final connection = _connection;
    if (state.isLoading ||
        previous == null ||
        connection == null ||
        !_isCurrent(connection) ||
        _sameConnection(_startingConnection, connection) ||
        previous.status.isRunning) {
      return false;
    }
    final generation = _generation;
    final sequence = ++_startSequence;
    _startingConnection = connection;
    state = AsyncData(
      BackgroundSyncState(
        previous.status,
        connection: connection,
        isStarting: true,
      ),
    );
    try {
      final status = await ref
          .read(backgroundSyncRepositoryProvider)
          .start(connection);
      if (!_isCurrent(connection)) return false;
      if (generation == _generation) {
        state = AsyncData(BackgroundSyncState(status, connection: connection));
      } else {
        ref.invalidateSelf();
      }
      return true;
    } catch (error) {
      if (!_isCurrent(connection)) return false;
      if (generation == _generation) {
        state = AsyncData(
          BackgroundSyncState(
            state.value?.status ?? previous.status,
            connection: connection,
            requestError: error,
          ),
        );
      }
      await _forgetUnauthorized(connection, error);
      return false;
    } finally {
      if (sequence == _startSequence) _startingConnection = null;
      if (_isCurrent(connection)) {
        final current = state.value;
        if (current != null && current.isStarting && !state.isLoading) {
          state = AsyncData(
            BackgroundSyncState(
              current.status,
              connection: connection,
              requestError: current.requestError,
            ),
          );
        }
      }
    }
  }
}
