import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';

import '../../support/fake_desktop.dart';

/// 끊기에는 뜻이 다른 두 길이 있다.
///
/// 사용자가 누른 끊기는 데스크탑의 기기 목록까지 지워야 끝나고,
/// 데스크탑이 이미 토큰을 거절한 경우는 기기에 남은 값만 지우면 된다.
/// 죽은 토큰으로 데스크탑을 다시 부르면 통하지도 않고 두 뜻이 뒤섞인다.
/// 401 쪽 길은 앱 전체를 띄워야 드러나므로 `app_test.dart`가 지킨다.
ProviderContainer containerWith(
  FakeDesktopClient client,
  FakeConnectionStore store,
) {
  final container = ProviderContainer(
    overrides: [
      desktopClientProvider.overrideWithValue(client),
      connectionStoreProvider.overrideWithValue(store),
    ],
  );
  addTearDown(container.dispose);
  return container;
}

void main() {
  test('사용자가 끊으면 데스크탑에 먼저 알리고 기기에서도 지운다', () async {
    final client = FakeDesktopClient();
    final store = FakeConnectionStore();
    final container = containerWith(client, store);
    await container.read(connectionControllerProvider.future);

    final toldDesktop = await container
        .read(connectionControllerProvider.notifier)
        .disconnect();

    expect(toldDesktop, isTrue);
    expect(
      client.unpairCalls.single.token,
      'test-token',
      reason: '기기에서만 지우면 데스크탑은 그 토큰을 계속 통과시킨다',
    );
    expect(await store.read(), isNull);
  });

  test('데스크탑에 알리지 못해도 기기에 남은 열쇠는 지우고 실패를 돌려준다', () async {
    final client = FakeDesktopClient(
      unpairError: const DesktopRequestException(503),
    );
    final store = FakeConnectionStore();
    final container = containerWith(client, store);
    await container.read(connectionControllerProvider.future);

    final toldDesktop = await container
        .read(connectionControllerProvider.notifier)
        .disconnect();

    expect(toldDesktop, isFalse, reason: '데스크탑이 아직 이 기기를 들고 있다고 화면이 말해야 한다');
    expect(
      await store.read(),
      isNull,
      reason: '여기서 멈추면 앱은 못 쓰는 토큰을 든 채 페어링 화면에도 못 간다',
    );
  });

  /// 401은 데스크탑에 이 토큰의 기기가 이미 없다는 뜻이다.
  /// 실패로 접으면 설정 화면에서 먼저 끊은 사용자가 이미 끝난 뒤처리를 하라는 안내를 매번 받는다.
  test('데스크탑이 이미 이 기기를 지웠으면 알린 것으로 센다', () async {
    final client = FakeDesktopClient(
      unpairError: const DesktopRequestException(401),
    );
    final store = FakeConnectionStore();
    final container = containerWith(client, store);
    await container.read(connectionControllerProvider.future);

    final toldDesktop = await container
        .read(connectionControllerProvider.notifier)
        .disconnect();

    expect(toldDesktop, isTrue, reason: '사용자에게 시킬 뒤처리가 남아 있지 않다');
    expect(await store.read(), isNull);
  });
}
