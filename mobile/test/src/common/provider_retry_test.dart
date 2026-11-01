import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/task_detail/domain/mirror_pane.dart';

import '../../support/fake_desktop.dart';

/// provider가 던진 실패가 화면까지 곧바로 흘러가는지 본다.
///
/// Riverpod 3은 아무것도 시키지 않으면 실패한 provider를 열 번까지 되불러 가고, 그동안 상태를
/// `AsyncLoading`으로 둔다. 첫 되부름이 200ms 뒤이므로 그보다 짧게 기다렸을 때 이미 오류여야
/// 되부르지 않는다는 뜻이 된다. 기다리는 값이 첫 되부름보다 짧은 것이 이 테스트의 전부다.
const _beforeFirstRetry = Duration(milliseconds: 50);

ProviderContainer containerWith(
  FakeDesktopClient client, {
  FakeConnectionStore? store,
}) {
  final container = ProviderContainer(
    overrides: [
      desktopClientProvider.overrideWithValue(client),
      connectionStoreProvider.overrideWithValue(store ?? FakeConnectionStore()),
    ],
  );
  addTearDown(container.dispose);
  return container;
}

void main() {
  test('목록을 다시 불러오는 사이에 시작한 이전 추가 요청은 POST를 보내지 않는다', () async {
    final client = FakeDesktopClient(
      onCreateTerminalTab: () async =>
          (surfaces: const TaskSurfaces(tabs: []), paneId: 'stale'),
    );
    final container = containerWith(client);
    container.listen(surfacesProvider('task-1'), (_, _) {});
    await container.read(surfacesProvider('task-1').future);
    final creating = container
        .read(surfacesProvider('task-1').notifier)
        .createTab();
    container.invalidate(surfacesProvider('task-1'));
    await container.read(surfacesProvider('task-1').future);
    expect(await creating, isNull);
    expect(client.createdTabCalls, isEmpty);
  });

  test('PC를 바꾸면 이전 PC의 늦은 탭 추가 응답이 새 목록을 덮지 않는다', () async {
    const a = DesktopConnection(host: 'pc-a', port: 9736, token: 'token-a');
    const b = DesktopConnection(host: 'pc-b', port: 9736, token: 'token-b');
    final pending = Completer<({TaskSurfaces surfaces, String paneId})>();
    final client = FakeDesktopClient(onCreateTerminalTab: () => pending.future);
    final container = containerWith(
      client,
      store: FakeConnectionStore(
        initial: const SavedConnections(
          connections: [a, b],
          activeId: 'pc-a:9736',
        ),
      ),
    );
    container.listen(surfacesProvider('task-1'), (_, _) {});
    await container.read(surfacesProvider('task-1').future);
    final creating = container
        .read(surfacesProvider('task-1').notifier)
        .createTab();
    await Future<void>.delayed(Duration.zero);
    expect(client.createdTabCalls.single.connectionId, a.id);
    await container.read(connectionControllerProvider.notifier).select(b.id);
    await container.read(surfacesProvider('task-1').future);
    pending.complete((
      surfaces: const TaskSurfaces(tabs: [], canResize: true),
      paneId: 'old-pane',
    ));
    expect(await creating, isNull);
    expect(
      container
          .read(surfacesProvider('task-1'))
          .requireValue
          .surfaces
          .canResize,
      isFalse,
    );
  });

  test('탭 추가 토큰이 거절되면 기존 정책대로 해당 PC를 잊는다', () async {
    final client = FakeDesktopClient(
      onCreateTerminalTab: () async => throw const DesktopRequestException(401),
    );
    final container = containerWith(client);
    container.listen(surfacesProvider('task-1'), (_, _) {});
    await container.read(surfacesProvider('task-1').future);
    await expectLater(
      container.read(surfacesProvider('task-1').notifier).createTab(),
      throwsA(isA<DesktopRequestException>()),
    );
    expect(
      (await container.read(connectionControllerProvider.future)).connections,
      isEmpty,
    );
  });

  test('세션이 없다는 것은 되불러 가지 않고 곧바로 오류가 된다', () async {
    final container = containerWith(
      FakeDesktopClient(
        surfacesError: const SurfaceUnavailableException(
          SurfaceUnavailableReason.noSession,
        ),
      ),
    );
    container.listen(surfacesProvider('task-1'), (_, _) {});

    await Future<void>.delayed(_beforeFirstRetry);

    expect(
      container.read(surfacesProvider('task-1')),
      isA<AsyncError<TaskSurfacesState>>(),
      reason: '되부르는 동안 AsyncLoading으로 남으면 화면은 안내 대신 회전만 보여 준다',
    );
  });

  test('데스크탑에 닿지 못한 보드도 되불러 가지 않는다', () async {
    final container = containerWith(
      FakeDesktopClient(boardError: Exception('연결 실패')),
    );
    container.listen(boardProvider, (_, _) {});

    await Future<void>.delayed(_beforeFirstRetry);

    expect(container.read(boardProvider), isA<AsyncError<Object>>());
  });
}
