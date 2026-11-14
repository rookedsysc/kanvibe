import 'dart:async';
import 'dart:convert';

import 'package:http/http.dart' as http;
import 'package:http/testing.dart';

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
  DesktopClient client, {
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
  test('initial surfaces request omits optional metadata', () async {
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        expect(request.url.queryParameters['metadata'], 'minimal');
        return http.Response(
          jsonEncode({
            'surfaces': {'tabs': []},
          }),
          200,
        );
      }),
    );
    addTearDown(client.close);
    final container = containerWith(client);
    container.listen(surfacesProvider('task-1'), (_, _) {});
    await container.read(surfacesProvider('task-1').future);
  });

  test(
    'optional metadata is deduplicated and never blocks usable surfaces',
    () async {
      const initial = TaskSurfaces(tabs: [], canResize: true);
      final pending = Completer<TaskSurfaces>();
      var calls = 0;
      final client = FakeDesktopClient()
        ..onFetchSurfaces = () async {
          calls += 1;
          return calls == 1 ? initial : pending.future;
        };
      final container = containerWith(client);
      container.listen(surfacesProvider('task-1'), (_, _) {});
      await container.read(surfacesProvider('task-1').future);
      final controller = container.read(surfacesProvider('task-1').notifier);
      final first = controller.enrichMetadata();
      final second = controller.enrichMetadata();
      await Future<void>.delayed(Duration.zero);
      expect(calls, 2);
      expect(
        container.read(surfacesProvider('task-1')).requireValue.surfaces,
        same(initial),
      );
      pending.completeError(Exception('optional process-info failed'));
      await Future.wait([first, second]);
      expect(
        container.read(surfacesProvider('task-1')).requireValue.surfaces,
        same(initial),
      );
    },
  );

  test('successful optional enrichment updates terminal labels', () async {
    final initial = TaskSurfaces.fromJson({
      'tabs': [
        {
          'id': 'tab',
          'panes': [
            {'id': 'pane', 'command': ''},
          ],
        },
      ],
    });
    final enriched = TaskSurfaces.fromJson({
      'tabs': [
        {
          'id': 'tab',
          'panes': [
            {'id': 'pane', 'command': 'codex'},
          ],
        },
      ],
    });
    var calls = 0;
    final client = FakeDesktopClient()
      ..onFetchSurfaces = () async => ++calls == 1 ? initial : enriched;
    final container = containerWith(client);
    container.listen(surfacesProvider('task-1'), (_, _) {});
    await container.read(surfacesProvider('task-1').future);
    await container.read(surfacesProvider('task-1').notifier).enrichMetadata();
    expect(
      container
          .read(surfacesProvider('task-1'))
          .requireValue
          .surfaces
          .allPanes
          .single
          .command,
      'codex',
    );
    expect(client.surfaceRequests, [
      (minimal: true, tabId: null),
      (minimal: false, tabId: null),
    ]);
  });

  test('required geometry rejects placeholder rectangles without replacing usable state', () async {
    const initial = TaskSurfaces(tabs: [], geometryTabId: 'first');
    final invalid = TaskSurfaces.fromJson({
      'geometryTabId': 'second',
      'tabs': [
        {
          'id': 'second',
          'panes': [
            {'id': 'pane', 'width': 0, 'height': 0},
          ],
        },
      ],
    });
    var calls = 0;
    final client = FakeDesktopClient()
      ..onFetchSurfaces = () async => ++calls == 1 ? initial : invalid;
    final container = containerWith(client);
    container.listen(surfacesProvider('task-1'), (_, _) {});
    await container.read(surfacesProvider('task-1').future);
    await expectLater(
      container
          .read(surfacesProvider('task-1').notifier)
          .loadTabGeometry('second'),
      throwsA(isA<SurfaceUnavailableException>()),
    );
    expect(
      container.read(surfacesProvider('task-1')).requireValue.surfaces,
      same(initial),
    );
    expect(client.surfaceRequests.last, (minimal: true, tabId: 'second'));
  });

  test('late optional metadata cannot replace a tab mutation result', () async {
    const initial = TaskSurfaces(tabs: []);
    const changed = TaskSurfaces(tabs: [], canResize: true);
    final pending = Completer<TaskSurfaces>();
    var calls = 0;
    final client = FakeDesktopClient(
      onCreateTerminalTab: () async => (surfaces: changed, paneId: 'new'),
    )..onFetchSurfaces = () async => ++calls == 1 ? initial : pending.future;
    final container = containerWith(client);
    container.listen(surfacesProvider('task-1'), (_, _) {});
    await container.read(surfacesProvider('task-1').future);
    final controller = container.read(surfacesProvider('task-1').notifier);
    final enriching = controller.enrichMetadata();
    await Future<void>.delayed(Duration.zero);
    await controller.createTab();
    pending.complete(const TaskSurfaces(tabs: [], canRefreshHistory: true));
    await enriching;
    expect(
      container.read(surfacesProvider('task-1')).requireValue.surfaces,
      same(changed),
    );
  });

  test('late optional metadata cannot replace a rebuilt task state', () async {
    const initial = TaskSurfaces(tabs: []);
    const rebuilt = TaskSurfaces(tabs: [], canResize: true);
    final pending = Completer<TaskSurfaces>();
    var calls = 0;
    final client = FakeDesktopClient()
      ..onFetchSurfaces = () async {
        calls += 1;
        return calls == 1
            ? initial
            : calls == 2
            ? pending.future
            : rebuilt;
      };
    final container = containerWith(client);
    container.listen(surfacesProvider('task-1'), (_, _) {});
    await container.read(surfacesProvider('task-1').future);
    final controller = container.read(surfacesProvider('task-1').notifier);
    final enriching = controller.enrichMetadata();
    await Future<void>.delayed(Duration.zero);
    container.invalidate(surfacesProvider('task-1'));
    await container.read(surfacesProvider('task-1').future);
    pending.complete(const TaskSurfaces(tabs: [], canRefreshHistory: true));
    await enriching;
    expect(
      container.read(surfacesProvider('task-1')).requireValue.surfaces,
      same(rebuilt),
    );
  });

  test('pending tab creation rejects shortcut dispatch', () async {
    final pending = Completer<({TaskSurfaces surfaces, String paneId})>();
    final client = FakeDesktopClient(onCreateTerminalTab: () => pending.future);
    final container = containerWith(client);
    container.listen(surfacesProvider('task-1'), (_, _) {});
    await container.read(surfacesProvider('task-1').future);
    final controller = container.read(surfacesProvider('task-1').notifier);
    final creating = controller.createTab();
    expect(await controller.runShortcut('%19', 'split-right'), isNull);
    expect(client.shortcutActions, isEmpty);
    pending.complete((surfaces: const TaskSurfaces(tabs: []), paneId: '%20'));
    expect(await creating, '%20');
  });

  test(
    'shortcut and tab creation never overwrite concurrent surface mutations',
    () async {
      final pending = Completer<({TaskSurfaces surfaces, String? screen})>();
      final client = FakeDesktopClient(
        onRunTerminalShortcut: () => pending.future,
        onCreateTerminalTab: () async =>
            (surfaces: const TaskSurfaces(tabs: []), paneId: '%20'),
      );
      final container = containerWith(client);
      container.listen(surfacesProvider('task-1'), (_, _) {});
      await container.read(surfacesProvider('task-1').future);
      final controller = container.read(surfacesProvider('task-1').notifier);
      final running = controller.runShortcut('%19', 'split-right');
      await Future<void>.delayed(Duration.zero);
      expect(await controller.createTab(), isNull);
      expect(client.createdTabCalls, isEmpty);
      pending.complete((surfaces: const TaskSurfaces(tabs: []), screen: null));
      await running;
      expect(await controller.createTab(), isNotNull);
      expect(client.createdTabCalls, hasLength(1));
    },
  );

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
