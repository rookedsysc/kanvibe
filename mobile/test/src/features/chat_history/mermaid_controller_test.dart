import 'dart:convert';
import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/chat_history/application/mermaid_controller.dart';
import 'package:kanvibe_mobile/src/features/chat_history/domain/mermaid_image.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';

import '../ai_accounts/ai_test_support.dart'
    show AiMemoryConnections, fixturePc, otherPc;
import 'mermaid_test_support.dart';

const target = (
  definition: 'flowchart LR\nA --> B',
  theme: MermaidTheme.dark,
  width: 400,
);
void main() {
  test('동일 정의 구독은 합치고 theme와 width 변경은 별도로 요청한다', () async {
    var count = 0;
    final client = DesktopClient(
      httpClient: MockClient((r) async {
        count++;
        return mermaidResponse(
          imageEnvelope(width: (jsonDecode(r.body) as Map)['width'] as int),
        );
      }),
    );
    final c = ProviderContainer(
      overrides: [
        desktopClientProvider.overrideWithValue(client),
        connectionStoreProvider.overrideWithValue(AiMemoryConnections()),
      ],
    );
    addTearDown(() {
      c.dispose();
      client.close();
    });
    final p = mermaidImageProvider(target);
    c.listen(p, (_, _) {});
    c.listen(p, (_, _) {});
    await c.read(p.future);
    expect(count, 1);
    final light = mermaidImageProvider((
      definition: target.definition,
      theme: MermaidTheme.light,
      width: 400,
    ));
    c.listen(light, (_, _) {});
    await c.read(light.future);
    expect(count, 2);
    final wide = mermaidImageProvider((
      definition: target.definition,
      theme: MermaidTheme.light,
      width: 600,
    ));
    c.listen(wide, (_, _) {});
    expect((await c.read(wide.future)).width, 600);
    expect(count, 3);
  });
  test('PC 변경 후 기존 PNG 응답은 새 PC 이미지를 덮지 않는다', () async {
    final pending = Completer<http.Response>();
    var started = false;
    final client = DesktopClient(
      httpClient: MockClient((r) async {
        if (r.url.host == 'fixture') {
          started = true;
          return pending.future;
        }
        return mermaidResponse(imageEnvelope(height: 200));
      }),
    );
    final c = ProviderContainer(
      overrides: [
        desktopClientProvider.overrideWithValue(client),
        connectionStoreProvider.overrideWithValue(AiMemoryConnections()),
      ],
    );
    addTearDown(() {
      c.dispose();
      client.close();
    });
    final p = mermaidImageProvider(target);
    c.listen(p, (_, _) {});
    while (!started) {
      await Future<void>.delayed(Duration.zero);
    }
    await c.read(connectionControllerProvider.notifier).select(otherPc.id);
    expect((await c.read(p.future)).height, 200);
    pending.complete(mermaidResponse(imageEnvelope()));
    await Future<void>.delayed(Duration.zero);
    expect(c.read(p).requireValue.height, 200);
  });
  test('같은 PC token 교체 후 기존401은 새 pairing을 지우지 않는다', () async {
    final pending = Completer<http.Response>();
    var started = false;
    final store = AiMemoryConnections();
    final client = DesktopClient(
      httpClient: MockClient((r) async {
        if (r.headers['authorization'] == 'Bearer paired') {
          started = true;
          return pending.future;
        }
        return mermaidResponse(imageEnvelope(height: 220));
      }),
    );
    final c = ProviderContainer(
      overrides: [
        desktopClientProvider.overrideWithValue(client),
        connectionStoreProvider.overrideWithValue(store),
      ],
    );
    addTearDown(() {
      c.dispose();
      client.close();
    });
    final p = mermaidImageProvider(target);
    c.listen(p, (_, _) {});
    while (!started) {
      await Future<void>.delayed(Duration.zero);
    }
    store.registry = SavedConnections(
      connections: [
        DesktopConnection(
          host: fixturePc.host,
          port: fixturePc.port,
          token: 'replacement',
        ),
      ],
      activeId: fixturePc.id,
    );
    c.invalidate(connectionControllerProvider);
    expect((await c.read(p.future)).height, 220);
    pending.complete(
      mermaidResponse({'success': false, 'error': 'expired'}, status: 401),
    );
    await Future<void>.delayed(Duration.zero);
    expect(
      c.read(connectionControllerProvider).requireValue.activeConnection?.token,
      'replacement',
    );
    expect(c.read(p).requireValue.height, 220);
  });
}
