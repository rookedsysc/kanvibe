import 'dart:convert';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/features/connection/data/connection_store.dart';

void main() {
  test('기존 한 PC 연결을 목록으로 옮기고 이전 키는 새 값 저장 후 지운다', () async {
    FlutterSecureStorage.setMockInitialValues({
      'kanvibe_desktop_connection': jsonEncode({
        'host': '192.168.0.10',
        'port': 9736,
        'token': 'old-token',
      }),
    });
    const storage = FlutterSecureStorage();
    final store = ConnectionStore(storage: storage);

    final migrated = await store.readRegistry();

    expect(migrated.connections, hasLength(1));
    expect(migrated.activeConnection?.token, 'old-token');
    expect(migrated.activeConnection?.displayName, '192.168.0.10');
    expect(await storage.read(key: 'kanvibe_desktop_connection'), isNull);
    expect(await storage.read(key: 'kanvibe_desktop_connections'), isNotNull);
  });

  test('두 PC의 이름과 토큰, 현재 선택을 재시작 후 복원한다', () async {
    FlutterSecureStorage.setMockInitialValues({});
    const storage = FlutterSecureStorage();
    final store = ConnectionStore(storage: storage);
    const pcA = DesktopConnection(
      host: '192.168.0.10',
      port: 9736,
      token: 'token-a',
      name: '작업실',
    );
    const pcB = DesktopConnection(
      host: '192.168.0.11',
      port: 19736,
      token: 'token-b',
      name: '집 PC',
    );

    await store.writeRegistry((await store.readRegistry()).add(pcA).add(pcB));
    final restored = await ConnectionStore(storage: storage).readRegistry();

    expect(restored.connections.map((pc) => pc.name), ['작업실', '집 PC']);
    expect(restored.connections.map((pc) => pc.token), ['token-a', 'token-b']);
    expect(restored.activeConnection?.id, pcB.id);
  });
}
