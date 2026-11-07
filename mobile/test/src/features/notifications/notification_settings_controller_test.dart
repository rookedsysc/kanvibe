import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/notifications/application/notification_settings_controller.dart';
import 'package:kanvibe_mobile/src/features/notifications/domain/notification_settings.dart';

import '../../../support/fake_desktop.dart';

class DelayedSettingsClient extends FakeDesktopClient {
  final pending = Completer<NotificationSettings>();
  int saves = 0;

  @override
  Future<NotificationSettings> updateNotificationSettings(
    DesktopConnection connection, {
    required bool isEnabled,
    required Set<String> enabledStatuses,
  }) {
    saves += 1;
    return pending.future;
  }
}

void main() {
  const pcA = DesktopConnection(host: '127.0.0.1', port: 9736, token: 'a');
  const pcB = DesktopConnection(host: '127.0.0.2', port: 9736, token: 'b');

  ProviderContainer createContainer(DelayedSettingsClient client) {
    final container = ProviderContainer(
      overrides: [
        desktopClientProvider.overrideWithValue(client),
        connectionStoreProvider.overrideWithValue(
          FakeConnectionStore(
            initial: const SavedConnections(
              connections: [pcA, pcB],
              activeId: '127.0.0.1:9736',
            ),
          ),
        ),
      ],
    );
    container.listen(notificationSettingsProvider, (_, _) {});
    return container;
  }

  test('저장 중 중복 변경을 막고 완료 뒤 서버 값을 표시한다', () async {
    final client = DelayedSettingsClient();
    final container = createContainer(client);
    addTearDown(container.dispose);
    await container.read(notificationSettingsProvider.future);
    final controller = container.read(notificationSettingsProvider.notifier);
    final save = controller.save(isEnabled: false);
    expect(
      container.read(notificationSettingsProvider).requireValue.isSaving,
      isTrue,
    );
    expect(await controller.save(enabledStatuses: {}), isFalse);
    expect(client.saves, 1);
    client.pending.complete(
      NotificationSettings(isEnabled: false, enabledStatuses: ['review']),
    );
    expect(await save, isTrue);
    expect(
      container.read(notificationSettingsProvider).requireValue.isEnabled,
      isFalse,
    );
  });

  test('다른 PC를 선택한 뒤 도착한 저장 응답은 현재 PC 설정을 덮지 않는다', () async {
    final client = DelayedSettingsClient();
    final container = createContainer(client);
    addTearDown(container.dispose);
    await container.read(notificationSettingsProvider.future);
    final save = container
        .read(notificationSettingsProvider.notifier)
        .save(isEnabled: false);
    await container.read(connectionControllerProvider.notifier).select(pcB.id);
    await container.read(notificationSettingsProvider.future);
    client.pending.complete(
      NotificationSettings(isEnabled: false, enabledStatuses: []),
    );
    expect(await save, isFalse);
    expect(
      container.read(notificationSettingsProvider).requireValue.isEnabled,
      isTrue,
    );
    expect(
      container.read(notificationSettingsProvider).requireValue.isSaving,
      isFalse,
    );
  });

  test('화면이 해제된 뒤 저장 응답이 와도 폐기한다', () async {
    final client = DelayedSettingsClient();
    final container = createContainer(client);
    await container.read(notificationSettingsProvider.future);
    final save = container
        .read(notificationSettingsProvider.notifier)
        .save(isEnabled: false);
    container.dispose();
    client.pending.complete(
      NotificationSettings(isEnabled: false, enabledStatuses: []),
    );
    expect(await save, isFalse);
  });
}
