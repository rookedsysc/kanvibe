import 'dart:async';
import 'dart:convert';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/diff/application/diff_controller.dart';

import '../../../support/fake_desktop.dart';

void main() {
  test(
    'PC switch while a save is pending discards its late response',
    () async {
      final pending = Completer<http.Response>();
      final client = DesktopClient(
        httpClient: MockClient((request) async {
          if (request.method == 'PUT') return pending.future;
          return http.Response(
            jsonEncode({
              'success': true,
              'original': 'base',
              'content': request.url.host,
            }),
            200,
          );
        }),
      );
      final container = ProviderContainer(
        overrides: [
          desktopClientProvider.overrideWithValue(client),
          connectionStoreProvider.overrideWithValue(
            FakeConnectionStore(
              initial: const SavedConnections(
                connections: [
                  DesktopConnection(host: 'a', port: 4885, token: 'a'),
                  DesktopConnection(host: 'b', port: 4885, token: 'b'),
                ],
                activeId: 'a:4885',
              ),
            ),
          ),
        ],
      );
      addTearDown(container.dispose);
      addTearDown(client.close);
      const target = (taskId: 'task', filePath: 'file.dart');
      final provider = diffDocumentProvider(target);
      final listener = container.listen(provider, (_, _) {});
      addTearDown(listener.close);
      expect((await container.read(provider.future)).content, 'a');
      final save = container.read(provider.notifier).save('old PC edit');
      await container
          .read(connectionControllerProvider.notifier)
          .select('b:4885');
      expect((await container.read(provider.future)).content, 'b');
      pending.complete(http.Response('{"success":true}', 200));
      expect(await save, isFalse);
      expect(container.read(provider).value!.content, 'b');
    },
  );
  test('failed save retains the loaded contents and oversized text is rejected before HTTP', () async {
    var writes = 0;
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        if (request.method == 'PUT') {
          writes++;
          return http.Response('{"success":false,"error":"failure"}', 409);
        }
        return http.Response(
          '{"success":true,"original":"base","content":"before"}',
          200,
        );
      }),
    );
    final container = ProviderContainer(
      overrides: [
        desktopClientProvider.overrideWithValue(client),
        connectionStoreProvider.overrideWithValue(
          FakeConnectionStore(
            initial: const SavedConnections(
              connections: [
                DesktopConnection(host: 'a', port: 4885, token: 'a'),
              ],
              activeId: 'a:4885',
            ),
          ),
        ),
      ],
    );
    addTearDown(container.dispose);
    addTearDown(client.close);
    const target = (taskId: 'task', filePath: 'file.dart');
    final subscription = container.listen(
      diffDocumentProvider(target),
      (_, _) {},
    );
    addTearDown(subscription.close);
    await container.read(diffDocumentProvider(target).future);
    await expectLater(
      container.read(diffDocumentProvider(target).notifier).save('new'),
      throwsA(isA<DesktopRequestException>()),
    );
    expect(
      container.read(diffDocumentProvider(target)).value!.content,
      'before',
    );
    await expectLater(
      container
          .read(diffDocumentProvider(target).notifier)
          .save('x' * (1024 * 1024 + 1)),
      throwsArgumentError,
    );
    expect(writes, 1);
  });
}
