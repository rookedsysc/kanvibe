import 'dart:convert';
import 'dart:io';
import 'dart:ui' as ui;
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/features/chat_history/data/mermaid_repository.dart';
import 'package:kanvibe_mobile/src/features/chat_history/domain/mermaid_image.dart';

import '../ai_accounts/ai_test_support.dart' show fixturePc;
import 'mermaid_test_support.dart';

const target = (
  definition: 'flowchart LR\n A --> B',
  theme: MermaidTheme.dark,
  width: 400,
);
void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  test('paired POST 계약과 PNG IHDR을 소비하고 bytes를 읽기 전용으로 보관한다', () async {
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        expect(request.method, 'POST');
        expect(request.url.path, '/api/mobile/diagrams/render');
        expect(request.headers['authorization'], 'Bearer paired');
        expect(jsonDecode(request.body), {
          'definition': target.definition,
          'theme': 'dark',
          'width': 400,
        });
        return mermaidResponse(imageEnvelope());
      }),
    );
    addTearDown(client.close);
    final image = await MermaidRepository(client).render(fixturePc, target);
    expect(image.width, 400);
    expect(image.height, 160);
    expect(() => image.bytes[0] = 0, throwsUnsupportedError);
  });
  test('읽기 전용 render POST만 header 전 close를 한 번 복구한다', () async {
    var requests = 0;
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        requests++;
        if (requests == 1) {
          throw http.ClientException(
            'Connection closed before full header was received',
            request.url,
          );
        }
        return mermaidResponse(imageEnvelope());
      }),
    );
    addTearDown(client.close);
    expect(
      (await MermaidRepository(client).render(fixturePc, target)).width,
      400,
    );
    expect(requests, 2);
  });
  test('두 번째 header close는 전파하고 HTTP 422는 다시 보내지 않는다', () async {
    var requests = 0;
    var close = true;
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        requests++;
        if (close) {
          throw http.ClientException(
            'Connection closed before full header was received',
            request.url,
          );
        }
        return mermaidResponse({
          'success': false,
          'error': 'invalid Mermaid',
        }, status: 422);
      }),
    );
    addTearDown(client.close);
    await expectLater(
      MermaidRepository(client).render(fixturePc, target),
      throwsA(isA<http.ClientException>()),
    );
    expect(requests, 2);
    close = false;
    requests = 0;
    await expectLater(
      MermaidRepository(client).render(fixturePc, target),
      throwsA(
        isA<DesktopRequestException>().having(
          (e) => e.statusCode,
          'status',
          422,
        ),
      ),
    );
    expect(requests, 1);
  });
  test('정의 UTF8 한도와 폭 범위는 서버에 보내기 전에 거절한다', () async {
    var requests = 0;
    final client = DesktopClient(
      httpClient: MockClient((_) async {
        requests++;
        return mermaidResponse(imageEnvelope());
      }),
    );
    addTearDown(client.close);
    final repo = MermaidRepository(client);
    for (final invalid in [
      (definition: '', theme: MermaidTheme.dark, width: 400),
      (
        definition: List.filled(22000, '가').join(),
        theme: MermaidTheme.light,
        width: 400,
      ),
      (definition: 'graph LR', theme: MermaidTheme.dark, width: 319),
      (definition: 'graph LR', theme: MermaidTheme.dark, width: 1201),
    ]) {
      expect(() => repo.render(fixturePc, invalid), throwsArgumentError);
    }
    expect(requests, 0);
  });
  test('PNG magic, IHDR, IEND, 메타데이터 불일치와 byte/height 한도를 거절한다', () async {
    final valid = pngFixture();
    final badMagic = Uint8List.fromList(valid)..[0] = 0;
    final badHeader = Uint8List.fromList(valid)..[16] = 1;
    final badEnd = Uint8List.fromList(valid)..[valid.length - 1] = 0;
    final payloads = [
      imageEnvelope(bytes: badMagic),
      imageEnvelope(bytes: badHeader),
      imageEnvelope(bytes: badEnd),
      {...imageEnvelope(), 'width': 399},
      {...imageEnvelope(), 'height': 2401},
      {...imageEnvelope(), 'height': 1.5},
      {...imageEnvelope(), 'imageBase64': 'not base64'},
      {
        ...imageEnvelope(),
        'imageBase64': base64Encode(
          Uint8List(MermaidRepository.maxImageBytes + 1),
        ),
      },
    ];
    for (final payload in payloads) {
      final client = DesktopClient(
        httpClient: MockClient((_) async => mermaidResponse(payload)),
      );
      try {
        await expectLater(
          MermaidRepository(client).render(fixturePc, target),
          throwsFormatException,
        );
      } finally {
        client.close();
      }
    }
  });
  test('명시 JPEG 계약을 소비하고 실제 코덱으로 읽기 전용 bytes를 디코딩한다', () async {
    final bytes = File('test/fixtures/mermaid.jpg').readAsBytesSync();
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        expect(jsonDecode(request.body), {
          'definition': target.definition,
          'theme': 'dark',
          'width': 400,
        });
        return mermaidResponse({
          ...imageEnvelope(bytes: bytes),
          'mimeType': 'image/jpeg',
        });
      }),
    );
    addTearDown(client.close);
    final image = await MermaidRepository(client).render(fixturePc, target);
    expect(image.width, 400);
    expect(image.height, 160);
    expect(() => image.bytes[0] = 0, throwsUnsupportedError);
    final codec = await ui.instantiateImageCodec(image.bytes);
    try {
      final frame = await codec.getNextFrame();
      expect(frame.image.width, 400);
      expect(frame.image.height, 160);
      frame.image.dispose();
    } finally {
      codec.dispose();
    }
  });

  test('MIME가 없던 PNG와 명시 image/png 응답을 계속 지원한다', () async {
    for (final payload in [
      imageEnvelope(),
      {...imageEnvelope(), 'mimeType': 'image/png'},
    ]) {
      expect((await consume(payload)).width, 400);
    }
  });

  test('알 수 없는 MIME, null, 실제 JPEG/PNG header와 다른 MIME를 거절한다', () async {
    final jpeg = File('test/fixtures/mermaid.jpg').readAsBytesSync();
    for (final payload in [
      {...imageEnvelope(), 'mimeType': 'image/svg+xml'},
      {...imageEnvelope(), 'mimeType': null},
      {...imageEnvelope(), 'mimeType': 1},
      {...imageEnvelope(), 'mimeType': 'image/jpeg'},
      {...imageEnvelope(bytes: jpeg), 'mimeType': 'image/png'},
      imageEnvelope(bytes: jpeg),
    ]) {
      await expectLater(consume(payload), throwsFormatException);
    }
  });

  test('JPEG SOI/EOI, SOF 크기/정밀도/성분/marker와 segment 경계를 검증한다', () async {
    final valid = File('test/fixtures/mermaid.jpg').readAsBytesSync();
    final sof = List.generate(
      valid.length - 1,
      (i) => i,
    ).firstWhere((i) => valid[i] == 0xff && valid[i + 1] == 0xc0);
    final sos = List.generate(
      valid.length - 1,
      (i) => i,
    ).firstWhere((i) => valid[i] == 0xff && valid[i + 1] == 0xda);
    final scanStart = sos + 2 + ByteData.sublistView(valid).getUint16(sos + 2);
    final mutations = <Uint8List>[
      Uint8List.fromList([...valid.take(scanStart), 0xff, 0xd9]),
      Uint8List.fromList(valid)..[0] = 0,
      Uint8List.fromList(valid)..[valid.length - 1] = 0,
      Uint8List.fromList(valid)..[sof + 1] = 0xc3,
      Uint8List.fromList(valid)..[sof + 1] = 0xc4,
      Uint8List.fromList(valid)..[sof + 4] = 12,
      Uint8List.fromList(valid)..[sof + 9] = 4,
      Uint8List.fromList(valid)..[sof + 7] = 0,
      Uint8List.fromList(valid)..[sof + 5] = 1,
      Uint8List.fromList(valid)
        ..[4] = 0xff
        ..[5] = 0xff,
      Uint8List.fromList(valid)
        ..[4] = 0
        ..[5] = 1,
      Uint8List.fromList(valid)
        ..[sof + 2] = 0
        ..[sof + 3] = 2,
    ];
    for (final cut in [0, 1, 3, 4, sof + 2, sof + 3, sof + 8]) {
      mutations.add(Uint8List.fromList([...valid.take(cut), 0xff, 0xd9]));
    }
    for (final bytes in mutations) {
      await expectLater(
        consume({...imageEnvelope(bytes: bytes), 'mimeType': 'image/jpeg'}),
        throwsFormatException,
      );
    }
  });
}

Future<MermaidImage> consume(Map<String, dynamic> payload) async {
  final client = DesktopClient(
    httpClient: MockClient((_) async => mermaidResponse(payload)),
  );
  try {
    return await MermaidRepository(client).render(fixturePc, target);
  } finally {
    client.close();
  }
}
