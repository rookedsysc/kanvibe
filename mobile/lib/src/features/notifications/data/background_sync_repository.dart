import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../domain/background_sync_status.dart';

class BackgroundSyncRepository {
  const BackgroundSyncRepository(this.client);

  final DesktopClient client;

  Future<BackgroundSyncStatus> fetch(DesktopConnection connection) async =>
      _parse(
        (await client.requestJson(connection, 'GET', [
          'sync',
          'status',
        ]))['status'],
      );

  Future<BackgroundSyncStatus> start(DesktopConnection connection) async =>
      _parse(
        (await client.requestJson(connection, 'POST', [
          'sync',
          'start',
        ]))['status'],
      );

  BackgroundSyncStatus _parse(Object? value) {
    final raw = value as Map;
    return BackgroundSyncStatus(
      phase: raw['phase'] as String,
      isRunning: raw['isRunning'] == true,
      completedStages: raw['completedStages'] as int,
      reviewNeeded: raw['reviewNeeded'] == true,
      boardUpdated: raw['boardUpdated'] == true,
      error: raw['error'] as String?,
    );
  }
}
