import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/features/task_detail/data/terminal_management_repository.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const connection = DesktopConnection(host: 'pc', port: 4885, token: 'paired');
  tearDown(
    () => TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(ClipboardImageReader.channel, null),
  );
  test(
    'native clipboard image bytes are read and missing images remain empty',
    () async {
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(ClipboardImageReader.channel, (call) async {
            expect(call.method, 'readImage');
            return {
              'mimeType': 'image/png',
              'bytes': Uint8List.fromList([137, 80, 78, 71]),
            };
          });
      final image = await const ClipboardImageReader().read();
      expect(image!.mimeType, 'image/png');
      expect(image.bytes, [137, 80, 78, 71]);
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(
            ClipboardImageReader.channel,
            (_) async => null,
          );
      expect(await const ClipboardImageReader().read(), isNull);
    },
  );
  test(
    'oversized or unsupported clipboard data is rejected before transport',
    () async {
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(
            ClipboardImageReader.channel,
            (_) async => {
              'mimeType': 'image/png',
              'bytes': Uint8List(2 * 1024 * 1024 + 1),
            },
          );
      await expectLater(
        const ClipboardImageReader().read(),
        throwsFormatException,
      );
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(
            ClipboardImageReader.channel,
            (_) async => {'mimeType': 'image/svg+xml', 'bytes': Uint8List(1)},
          );
      await expectLater(
        const ClipboardImageReader().read(),
        throwsFormatException,
      );
    },
  );
  test('rename reorder and clipboard paste use their explicit authenticated endpoints', () async {
    final requests = <http.Request>[];
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        requests.add(request);
        return http.Response('{"success":true}', 200);
      }),
    );
    addTearDown(client.close);
    final repository = TerminalManagementRepository(client);
    await repository.rename(connection, 'task', '@1', 'new name');
    expect(requests.last.method, 'PATCH');
    expect(jsonDecode(requests.last.body), {'name': 'new name'});
    await repository.move(connection, 'task', '@1', 2);
    expect(requests.last.url.path, '/api/mobile/tasks/task/tabs/@1/move');
    expect(jsonDecode(requests.last.body), {'index': 2});
    await repository.pasteImage(
      connection,
      'task',
      '%1',
      ClipboardImage(
        mimeType: 'image/png',
        bytes: Uint8List.fromList([137, 80, 78, 71]),
      ),
    );
    expect(requests.last.url.pathSegments, [
      'api',
      'mobile',
      'tasks',
      'task',
      'panes',
      '%1',
      'image',
    ]);
    expect(jsonDecode(requests.last.body), {
      'mimeType': 'image/png',
      'base64': 'iVBORw==',
    });
    expect(
      requests.every(
        (request) => request.headers['authorization'] == 'Bearer paired',
      ),
      isTrue,
    );
  });
}
