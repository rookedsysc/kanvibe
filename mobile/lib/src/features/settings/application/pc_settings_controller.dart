import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../../task_create/domain/session_type.dart';
import '../data/pc_settings_repository.dart';
import '../domain/pc_settings.dart';

final pcSettingsRepositoryProvider = Provider<PcSettingsRepository>(
  (ref) => PcSettingsRepository(ref.watch(desktopClientProvider)),
);
final pcSettingsProvider =
    AsyncNotifierProvider.autoDispose<PcSettingsController, PcSettingsState>(
      PcSettingsController.new,
      retry: (_, _) => null,
    );
final pcOperationsProvider =
    AsyncNotifierProvider.autoDispose<
      PcOperationsController,
      PcOperationResult?
    >(PcOperationsController.new, retry: (_, _) => null);
final sessionChangeTargetsProvider = FutureProvider.autoDispose
    .family<SessionChangeTargets, SessionType>((ref, type) async {
      final connection = (await ref.watch(connectionControllerProvider.future))
          .activeConnection;
      if (connection == null) throw StateError('연결된 PC가 없습니다.');
      try {
        return await ref
            .read(pcSettingsRepositoryProvider)
            .fetchTargets(connection, type);
      } on DesktopRequestException catch (error) {
        await _forgetUnauthorized(ref, connection, error);
        rethrow;
      }
    }, retry: (_, _) => null);

class PcSettingsState {
  const PcSettingsState(this.settings, {this.isSaving = false, this.error});
  final PcSettings settings;
  final bool isSaving;
  final Object? error;
}

bool _current(Ref ref, DesktopConnection connection) {
  if (!ref.mounted) return false;
  final active = ref.read(connectionControllerProvider).value?.activeConnection;
  return active?.id == connection.id && active?.token == connection.token;
}

Future<void> _forgetUnauthorized(
  Ref ref,
  DesktopConnection connection,
  Object error,
) async {
  if (error is DesktopRequestException &&
      error.isUnauthorized &&
      _current(ref, connection)) {
    await ref
        .read(connectionControllerProvider.notifier)
        .forgetLocally(connection.id, connection.token);
  }
}

class PcSettingsController extends AsyncNotifier<PcSettingsState> {
  int _generation = 0;
  DesktopConnection? _connection;
  @override
  Future<PcSettingsState> build() async {
    _generation++;
    final connection = (await ref.watch(connectionControllerProvider.future))
        .activeConnection;
    if (connection == null) throw StateError('연결된 PC가 없습니다.');
    _connection = connection;
    try {
      return PcSettingsState(
        await ref.read(pcSettingsRepositoryProvider).fetchSettings(connection),
      );
    } catch (error) {
      await _forgetUnauthorized(ref, connection, error);
      rethrow;
    }
  }

  Future<bool> save({
    SessionType? defaultSessionType,
    bool? isSyncEnabled,
    int? intervalMinutes,
  }) async {
    final previous = state.value;
    final connection = _connection;
    if (state.isLoading ||
        previous == null ||
        previous.isSaving ||
        connection == null) {
      return false;
    }
    final interval = intervalMinutes ?? previous.settings.intervalMinutes;
    if (interval < 1 || interval > 1440) return false;
    final generation = _generation;
    state = AsyncData(PcSettingsState(previous.settings, isSaving: true));
    try {
      final saved = await ref
          .read(pcSettingsRepositoryProvider)
          .saveSettings(
            connection,
            defaultSessionType: defaultSessionType,
            isSyncEnabled:
                isSyncEnabled ??
                (intervalMinutes != null
                    ? previous.settings.isSyncEnabled
                    : null),
            intervalMinutes: isSyncEnabled != null || intervalMinutes != null
                ? interval
                : null,
          );
      if (!_current(ref, connection) || generation != _generation) return false;
      state = AsyncData(PcSettingsState(saved));
      return true;
    } catch (error) {
      if (!_current(ref, connection) || generation != _generation) return false;
      state = AsyncData(PcSettingsState(previous.settings, error: error));
      await _forgetUnauthorized(ref, connection, error);
      return false;
    }
  }
}

class PcOperationsController extends AsyncNotifier<PcOperationResult?> {
  int _generation = 0;
  @override
  Future<PcOperationResult?> build() async {
    _generation++;
    await ref.watch(connectionControllerProvider.future);
    return null;
  }

  Future<bool> _run(
    Future<PcOperationResult> Function(PcSettingsRepository, DesktopConnection)
    operation,
  ) async {
    if (state.isLoading) return false;
    final connection = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    if (connection == null) return false;
    final generation = _generation;
    state = const AsyncLoading();
    try {
      final result = await operation(
        ref.read(pcSettingsRepositoryProvider),
        connection,
      );
      if (!_current(ref, connection) || generation != _generation) return false;
      state = AsyncData(result);
      ref.invalidate(boardProvider);
      ref.invalidate(sessionChangeTargetsProvider);
      return true;
    } catch (error, stack) {
      if (!_current(ref, connection) || generation != _generation) return false;
      state = AsyncError(error, stack);
      await _forgetUnauthorized(ref, connection, error);
      return false;
    }
  }

  Future<bool> changeSessions(SessionType type, bool closeExisting) => _run(
    (repository, connection) =>
        repository.changeSessions(connection, type, closeExisting),
  );
}
