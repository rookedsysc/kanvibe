import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../domain/notification_settings.dart';

final notificationSettingsProvider =
    AsyncNotifierProvider.autoDispose<
      NotificationSettingsController,
      NotificationSettings
    >(NotificationSettingsController.new, retry: (_, _) => null);

class NotificationSettingsController
    extends AsyncNotifier<NotificationSettings> {
  int _generation = 0;
  DesktopConnection? _connection;

  @override
  Future<NotificationSettings> build() async {
    final generation = ++_generation;
    final connection = (await ref.watch(connectionControllerProvider.future))
        .activeConnection;
    if (connection == null || !_isCurrent(connection, generation)) {
      throw StateError('연결된 PC가 없습니다.');
    }
    _connection = connection;
    try {
      return await ref
          .read(desktopClientProvider)
          .fetchNotificationSettings(connection);
    } on DesktopRequestException catch (error) {
      await _forgetRejectedConnection(error, connection, generation);
      rethrow;
    }
  }

  Future<bool> save({bool? isEnabled, Set<String>? enabledStatuses}) async {
    final previous = state.value;
    final connection = _connection;
    if (state.isLoading ||
        previous == null ||
        previous.isSaving ||
        connection == null) {
      return false;
    }
    final generation = _generation;
    state = AsyncData(
      NotificationSettings(
        isEnabled: previous.isEnabled,
        enabledStatuses: previous.enabledStatuses,
        isSaving: true,
      ),
    );
    try {
      final saved = await ref
          .read(desktopClientProvider)
          .updateNotificationSettings(
            connection,
            isEnabled: isEnabled ?? previous.isEnabled,
            enabledStatuses: enabledStatuses ?? previous.enabledStatuses,
          );
      if (!_isCurrent(connection, generation)) return false;
      state = AsyncData(saved);
      return true;
    } catch (error) {
      if (!_isCurrent(connection, generation)) return false;
      state = AsyncData(previous);
      if (error is DesktopRequestException) {
        await _forgetRejectedConnection(error, connection, generation);
      }
      return false;
    }
  }

  bool _isCurrent(DesktopConnection connection, int generation) {
    if (!ref.mounted || generation != _generation) return false;
    final active = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    return active?.id == connection.id && active?.token == connection.token;
  }

  Future<void> _forgetRejectedConnection(
    DesktopRequestException error,
    DesktopConnection connection,
    int generation,
  ) async {
    if (error.isUnauthorized && _isCurrent(connection, generation)) {
      await ref
          .read(connectionControllerProvider.notifier)
          .forgetLocally(connection.id, connection.token);
    }
  }
}
