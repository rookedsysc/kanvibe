import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/notifications/application/background_sync_controller.dart';
import 'package:kanvibe_mobile/src/features/notifications/application/mobile_notification_controller.dart';

import '../../../support/fake_desktop.dart';
import '../../../support/fake_notifications.dart';

const _pcA = DesktopConnection(host: 'a', port: 9736, token: 'a');
const _pcB = DesktopConnection(host: 'b', port: 9736, token: 'b');

Map<String, dynamic> _status(String phase, {bool reviewNeeded = false}) => {
  'phase': phase,
  'isRunning': ['worktrees', 'pull-requests', 'pulls'].contains(phase),
  'completedStages': phase == 'completed'
      ? 3
      : phase == 'pulls'
      ? 2
      : 0,
  'reviewNeeded': reviewNeeded,
  'boardUpdated': false,
  'error': null,
};

class _SyncClient extends FakeDesktopClient {
  final statuses = <String, Map<String, dynamic>>{
    _pcA.id: _status('idle'),
    _pcB.id: _status('idle'),
  };
  final fetches = <String>[];
  int starts = 0;
  Future<Map<String, dynamic>> Function()? onStart;
  Future<Map<String, dynamic>> Function()? onFetch;

  @override
  Future<Map<String, dynamic>> requestJson(
    DesktopConnection connection,
    String method,
    List<String> segments, {
    Map<String, String>? query,
    Map<String, dynamic>? body,
    Duration timeout = const Duration(seconds: 30),
  }) async {
    expect(segments.first, 'sync');
    if (method == 'POST') {
      expect(segments.last, 'start');
      starts++;
      return {'status': await onStart?.call() ?? _status('worktrees')};
    }
    expect(segments.last, 'status');
    fetches.add(connection.id);
    return {'status': await onFetch?.call() ?? statuses[connection.id]};
  }
}

void main() {
  late _SyncClient client;
  late FakeMobileNotifications native;
  late ProviderContainer container;
  setUp(() async {
    client = _SyncClient();
    native = FakeMobileNotifications();
    container = ProviderContainer(
      overrides: [
        desktopClientProvider.overrideWithValue(client),
        connectionStoreProvider.overrideWithValue(
          FakeConnectionStore(
            initial: SavedConnections(
              connections: [_pcA, _pcB],
              activeId: _pcA.id,
            ),
          ),
        ),
        mobileNotificationsProvider.overrideWithValue(native),
      ],
    );
    await container.read(backgroundSyncProvider.future);
  });
  tearDown(() async {
    container.dispose();
    await native.dispose();
  });

  test('동시 시작 요청은 한 번만 전송하고 서버 실행 상태를 유지한다', () async {
    final pending = Completer<Map<String, dynamic>>();
    client.onStart = () => pending.future;
    final controller = container.read(backgroundSyncProvider.notifier);
    final first = controller.start();
    expect(await controller.start(), isFalse);
    expect(
      container.read(backgroundSyncProvider).requireValue.isStarting,
      isTrue,
    );
    expect(client.starts, 1);
    pending.complete(_status('worktrees'));
    expect(await first, isTrue);
    expect(
      container.read(backgroundSyncProvider).requireValue.status.isRunning,
      isTrue,
    );
    expect(await controller.start(), isFalse);
    expect(client.starts, 1);
  });

  test('다른 PC의 이벤트를 무시하고 현재 PC 진행 및 완료를 다시 조회한다', () async {
    final before = client.fetches.length;
    client.openedEvents[_pcB.id]!.add({
      'type': 'background-sync-status-changed',
      'status': _status('pulls'),
    });
    await Future<void>(() {});
    expect(client.fetches.length, before);
    client.statuses[_pcA.id] = _status('pulls');
    client.openedEvents[_pcA.id]!.add({
      'type': 'background-sync-status-changed',
      'status': _status('pulls'),
    });
    await Future<void>(() {});
    expect(
      (await container.read(backgroundSyncProvider.future)).status.phase,
      'pulls',
    );
    client.statuses[_pcA.id] = _status('completed', reviewNeeded: true);
    client.openedEvents[_pcA.id]!.add({
      'type': 'background-sync-status-changed',
      'status': client.statuses[_pcA.id],
    });
    await Future<void>(() {});
    expect(
      (await container.read(backgroundSyncProvider.future)).status.reviewNeeded,
      isTrue,
    );
    expect(native.shown, isEmpty);
  });

  test('PC 변경 뒤 늦게 도착한 시작 응답은 새 PC 상태를 덮지 않는다', () async {
    final pending = Completer<Map<String, dynamic>>();
    client.onStart = () => pending.future;
    final first = container.read(backgroundSyncProvider.notifier).start();
    await container.read(connectionControllerProvider.notifier).select(_pcB.id);
    await container.read(backgroundSyncProvider.future);
    pending.complete(_status('pulls'));
    expect(await first, isFalse);
    expect(
      container.read(backgroundSyncProvider).requireValue.status.phase,
      'idle',
    );
    expect(
      container.read(backgroundSyncProvider).requireValue.isStarting,
      isFalse,
    );
  });

  test('시작 응답보다 최신 완료 이벤트가 먼저 도착해도 완료 상태를 보존한다', () async {
    final pending = Completer<Map<String, dynamic>>();
    client.onStart = () => pending.future;
    final first = container.read(backgroundSyncProvider.notifier).start();
    client.statuses[_pcA.id] = _status('completed');
    client.openedEvents[_pcA.id]!.add({
      'type': 'background-sync-status-changed',
      'status': _status('completed'),
    });
    await Future<void>(() {});
    await container.read(backgroundSyncProvider.future);
    pending.complete(_status('worktrees'));
    await first;
    expect(
      (await container.read(backgroundSyncProvider.future)).status.phase,
      'completed',
    );
  });

  test('시작 실패 후 다시 시도할 수 있고 재연결은 최신 상태를 복원한다', () async {
    client.onStart = () async =>
        throw const DesktopRequestException(500, message: 'sync failed');
    final controller = container.read(backgroundSyncProvider.notifier);
    expect(await controller.start(), isFalse);
    expect(
      container.read(backgroundSyncProvider).requireValue.requestError,
      isA<DesktopRequestException>(),
    );
    client.onStart = null;
    expect(await controller.start(), isTrue);
    client.statuses[_pcA.id] = _status('completed');
    client.openedEvents[_pcA.id]!.add({'type': 'connected'});
    await Future<void>(() {});
    expect(
      (await container.read(backgroundSyncProvider.future)).status.phase,
      'completed',
    );
  });

  test('시작 요청 대기 중 provider가 폐기되면 늦은 응답을 무시한다', () async {
    final pending = Completer<Map<String, dynamic>>();
    client.onStart = () => pending.future;
    final first = container.read(backgroundSyncProvider.notifier).start();
    container.dispose();
    pending.complete(_status('worktrees'));
    expect(await first, isFalse);
  });

  for (final rejects in [false, true]) {
    test('단계 조회 대기 중 시작 응답($rejects)이 와도 실행 잠금을 해제하지 않는다', () async {
      final startResponse = Completer<Map<String, dynamic>>();
      final refreshResponse = Completer<Map<String, dynamic>>();
      client.onStart = () => startResponse.future;
      client.onFetch = () => refreshResponse.future;
      final controller = container.read(backgroundSyncProvider.notifier);
      final first = controller.start();
      client.openedEvents[_pcA.id]!.add({
        'type': 'background-sync-status-changed',
      });
      await Future<void>(() {});
      container.read(backgroundSyncProvider);
      await Future<void>(() {});
      if (rejects) {
        startResponse.completeError(
          const DesktopRequestException(500, message: 'start reply failed'),
        );
      } else {
        startResponse.complete(_status('worktrees'));
      }
      expect(await first, !rejects);
      expect(container.read(backgroundSyncProvider).isLoading, isTrue);
      expect(await controller.start(), isFalse);
      expect(client.starts, 1);
      refreshResponse.complete(_status('worktrees'));
      expect(
        (await container.read(backgroundSyncProvider.future)).status.isRunning,
        isTrue,
      );
    });
  }
}
