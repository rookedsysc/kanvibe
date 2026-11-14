import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/settings/application/pc_settings_controller.dart';
import 'package:kanvibe_mobile/src/features/task_create/domain/session_type.dart';

import '../ai_accounts/ai_test_support.dart';

Map<String, dynamic> pcSettings({
  String type = 'herdr',
  bool enabled = true,
  int minutes = 10,
}) => {
  'success': true,
  'settings': {
    'defaultSessionType': type,
    'backgroundSync': {'isEnabled': enabled, 'intervalMs': minutes * 60000},
  },
};
ProviderContainer settingsContainer(AiStreamClient client) {
  final container = ProviderContainer(
    overrides: [
      desktopClientProvider.overrideWithValue(client),
      connectionStoreProvider.overrideWithValue(AiMemoryConnections()),
    ],
  );
  container.listen(pcSettingsProvider, (_, _) {});
  container.listen(pcOperationsProvider, (_, _) {});
  addTearDown(() {
    container.dispose();
    client.close();
  });
  return container;
}

void main() {
  test('설정 저장 중 연타를 막고 응답의 값을 적용한다', () async {
    final pending = Completer<http.Response>();
    var patches = 0;
    final client = AiStreamClient((request) async {
      if (request.method == 'PATCH') {
        patches++;
        return pending.future;
      }
      return aiResponse(pcSettings());
    });
    final container = settingsContainer(client);
    await container.read(pcSettingsProvider.future);
    final controller = container.read(pcSettingsProvider.notifier);
    final save = controller.save(defaultSessionType: SessionType.tmux);
    expect(await controller.save(isSyncEnabled: false), isFalse);
    pending.complete(aiResponse(pcSettings(type: 'tmux')));
    expect(await save, isTrue);
    expect(patches, 1);
    expect(
      container
          .read(pcSettingsProvider)
          .requireValue
          .settings
          .defaultSessionType,
      SessionType.tmux,
    );
  });
  test('주기 범위와 저장 실패는 이전 값을 지키고 재시도한다', () async {
    var fail = true;
    var writes = 0;
    final client = AiStreamClient((request) async {
      if (request.method == 'PATCH') {
        writes++;
        return fail
            ? aiResponse({'success': false, 'error': 'offline'}, status: 503)
            : aiResponse(pcSettings(minutes: 5));
      }
      return aiResponse(pcSettings());
    });
    final container = settingsContainer(client);
    await container.read(pcSettingsProvider.future);
    final controller = container.read(pcSettingsProvider.notifier);
    expect(await controller.save(intervalMinutes: 0), isFalse);
    expect(await controller.save(intervalMinutes: 1441), isFalse);
    expect(writes, 0);
    expect(await controller.save(intervalMinutes: 5), isFalse);
    expect(
      container.read(pcSettingsProvider).requireValue.settings.intervalMinutes,
      10,
    );
    fail = false;
    expect(await controller.save(intervalMinutes: 5), isTrue);
    expect(
      container.read(pcSettingsProvider).requireValue.settings.intervalMinutes,
      5,
    );
  });
  test('PC 전환 후 늦은 저장은 현재 설정을 덮지 않는다', () async {
    final pending = Completer<http.Response>();
    final client = AiStreamClient(
      (request) async => request.method == 'PATCH'
          ? pending.future
          : aiResponse(
              pcSettings(type: request.url.host == 'other' ? 'tmux' : 'herdr'),
            ),
    );
    final container = settingsContainer(client);
    await container.read(pcSettingsProvider.future);
    final save = container
        .read(pcSettingsProvider.notifier)
        .save(intervalMinutes: 5);
    await container
        .read(connectionControllerProvider.notifier)
        .select(otherPc.id);
    expect(
      (await container.read(pcSettingsProvider.future))
          .settings
          .defaultSessionType,
      SessionType.tmux,
    );
    pending.complete(aiResponse(pcSettings(minutes: 5)));
    expect(await save, isFalse);
    expect(
      container.read(pcSettingsProvider).requireValue.settings.intervalMinutes,
      10,
    );
  });
  test('동기화 실패와 재시도를 상태로 보존한다', () async {
    var fail = true;
    final client = AiStreamClient(
      (request) async => request.method == 'POST'
          ? fail
                ? aiResponse({
                    'success': false,
                    'error': 'sync failed',
                  }, status: 500)
                : aiResponse({
                    'success': true,
                    'result': {'reviewNeeded': true, 'boardUpdated': true},
                  })
          : aiResponse(pcSettings()),
    );
    final container = settingsContainer(client);
    await container.read(pcOperationsProvider.future);
    final controller = container.read(pcOperationsProvider.notifier);
    expect(await controller.runSync(), isFalse);
    expect(container.read(pcOperationsProvider).hasError, isTrue);
    fail = false;
    expect(await controller.runSync(), isTrue);
    expect(container.read(pcOperationsProvider).hasValue, isTrue);
  });
}
