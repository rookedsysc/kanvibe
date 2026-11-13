import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/constants/app_theme.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';

import '../ai_accounts/ai_test_support.dart' show AiMemoryConnections;

Uint8List pngFixture({int width = 400, int height = 160}) {
  int crc(List<int> bytes) {
    var result = 0xffffffff;
    for (final byte in bytes) {
      result ^= byte;
      for (var bit = 0; bit < 8; bit++) {
        result = (result & 1) != 0 ? 0xedb88320 ^ (result >> 1) : result >> 1;
      }
    }
    return result ^ 0xffffffff;
  }

  List<int> chunk(String type, List<int> data) {
    final payload = [...ascii.encode(type), ...data];
    final length = ByteData(4)..setUint32(0, data.length);
    final checksum = ByteData(4)..setUint32(0, crc(payload));
    return [
      ...length.buffer.asUint8List(),
      ...payload,
      ...checksum.buffer.asUint8List(),
    ];
  }

  final header = ByteData(13)
    ..setUint32(0, width)
    ..setUint32(4, height)
    ..setUint8(8, 8)
    ..setUint8(9, 2);
  final rows = <int>[
    for (var y = 0; y < height; y++) ...[0, ...List.filled(width * 3, 150)],
  ];
  return Uint8List.fromList([
    137,
    80,
    78,
    71,
    13,
    10,
    26,
    10,
    ...chunk('IHDR', header.buffer.asUint8List()),
    ...chunk('IDAT', ZLibEncoder().convert(rows)),
    ...chunk('IEND', []),
  ]);
}

Map<String, dynamic> imageEnvelope({
  int width = 400,
  int height = 160,
  Uint8List? bytes,
}) => {
  'success': true,
  'imageBase64': base64Encode(
    bytes ?? pngFixture(width: width, height: height),
  ),
  'width': width,
  'height': height,
};
http.Response mermaidResponse(Object body, {int status = 200}) => http.Response(
  jsonEncode(body),
  status,
  headers: {'content-type': 'application/json; charset=utf-8'},
);
Future<ProviderContainer> mountMermaid(
  WidgetTester tester,
  Widget widget,
  Future<http.Response> Function(http.Request) handler, {
  Brightness brightness = Brightness.dark,
}) async {
  final client = DesktopClient(httpClient: MockClient(handler));
  final container = ProviderContainer(
    overrides: [
      desktopClientProvider.overrideWithValue(client),
      connectionStoreProvider.overrideWithValue(AiMemoryConnections()),
    ],
  );
  await tester.pumpWidget(
    UncontrolledProviderScope(
      container: container,
      child: MaterialApp(
        theme: buildAppTheme(brightness),
        home: Scaffold(
          body: Center(child: SizedBox(width: 400, child: widget)),
        ),
      ),
    ),
  );
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 100));
  addTearDown(() async {
    await tester.pumpWidget(const SizedBox());
    container.dispose();
    client.close();
  });
  return container;
}

Future<void> settleMermaid(WidgetTester tester) async {
  // PNG 디코더는 위젯 테스트의 가상 시계 밖에서 실제 frame을 준비한다.
  for (var attempt = 0; attempt < 50; attempt++) {
    await tester.pump();
    await tester.runAsync(
      () => Future<void>.delayed(const Duration(milliseconds: 20)),
    );
    await tester.pump();
    if (find.byType(CircularProgressIndicator).evaluate().isEmpty) break;
  }
  await tester.pumpAndSettle();
}
