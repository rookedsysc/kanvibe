import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../../task_create/domain/session_type.dart';
import '../domain/pc_settings.dart';

class PcSettingsRepository {
  const PcSettingsRepository(this.client);
  final DesktopClient client;
  static const _millisecondsPerMinute = 60000;
  static const _operationTimeout = Duration(minutes: 5);
  Future<PcSettings> fetchSettings(DesktopConnection connection) async =>
      _parseSettings(
        (await client.requestJson(connection, 'GET', ['settings']))['settings'],
      );
  Future<PcSettings> saveSettings(
    DesktopConnection connection, {
    SessionType? defaultSessionType,
    bool? isSyncEnabled,
    int? intervalMinutes,
  }) async {
    final body = <String, dynamic>{
      if (defaultSessionType != null)
        'defaultSessionType': defaultSessionType.wireValue,
      if (isSyncEnabled != null && intervalMinutes != null)
        'backgroundSync': {
          'isEnabled': isSyncEnabled,
          'intervalMs': intervalMinutes * _millisecondsPerMinute,
        },
    };
    return _parseSettings(
      (await client.requestJson(connection, 'PATCH', [
        'settings',
      ], body: body))['settings'],
    );
  }

  Future<SessionChangeTargets> fetchTargets(
    DesktopConnection connection,
    SessionType type,
  ) async {
    final raw =
        (await client.requestJson(
              connection,
              'GET',
              ['sessions', 'change-targets'],
              query: {'sessionType': type.wireValue},
            ))['targets']
            as Map;
    return SessionChangeTargets(
      taskCount: raw['taskCount'] as int,
      liveSessionCount: raw['liveSessionCount'] as int,
      sshHosts: List<String?>.unmodifiable(raw['sshHosts'] as List),
    );
  }

  Future<BulkSessionChangeResult> changeSessions(
    DesktopConnection connection,
    SessionType type,
    bool shouldCloseExistingSessions,
  ) async {
    final raw =
        (await client.requestJson(
              connection,
              'POST',
              ['sessions', 'change-all'],
              body: {
                'sessionType': type.wireValue,
                'shouldCloseExistingSessions': shouldCloseExistingSessions,
              },
              timeout: _operationTimeout,
            ))['result']
            as Map;
    return BulkSessionChangeResult(
      changedCount: raw['changedCount'] as int,
      failedCount: raw['failedCount'] as int,
    );
  }
}

PcSettings _parseSettings(Object? value) {
  final raw = value as Map;
  final background = raw['backgroundSync'] as Map;
  final type = SessionType.values
      .where((type) => type.wireValue == raw['defaultSessionType'])
      .firstOrNull;
  if (type == null) throw const FormatException('지원하지 않는 기본 세션입니다.');
  return PcSettings(
    defaultSessionType: type,
    isSyncEnabled: background['isEnabled'] == true,
    intervalMinutes:
        (background['intervalMs'] as int) ~/
        PcSettingsRepository._millisecondsPerMinute,
  );
}
