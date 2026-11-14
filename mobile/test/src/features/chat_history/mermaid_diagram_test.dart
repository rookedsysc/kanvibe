import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:kanvibe_mobile/src/common/constants/app_theme.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';

import '../ai_accounts/ai_test_support.dart' show otherPc;

import 'package:kanvibe_mobile/src/features/chat_history/domain/chat_history.dart';
import 'package:kanvibe_mobile/src/features/chat_history/presentation/chat_message_tile.dart';
import 'package:kanvibe_mobile/src/features/chat_history/presentation/chat_attachment_tile.dart';
import 'package:kanvibe_mobile/src/features/chat_history/presentation/widgets/mermaid_diagram.dart';

import 'mermaid_test_support.dart';

const definition = 'flowchart LR\nA[Raw source only] --> B[No false PASS]';
Finder ready() => find.byWidgetPredicate(
  (w) => w is Semantics && w.properties.identifier == 'mermaid-image-ready',
);
void main() {
  testWidgets('새 대화 메시지는 인증된 이미지 첨부와 Mermaid 본문을 함께 표시한다', (t) async {
    final requests = <http.Request>[];
    await mountMermaid(
      t,
      const ChatMessageTile(
        provider: 'hermes',
        taskId: 'task',
        sessionId: 'session',
        message: ChatMessage(
          role: 'assistant',
          text: '# 결과\n\n```mermaid\n$definition\n```',
          attachments: [
            ChatAttachment(id: 'image', kind: 'image', name: 'shot.png'),
          ],
        ),
      ),
      (r) async {
        requests.add(r);
        if (r.url.path.contains('/attachments/')) {
          return mermaidResponse({
            'success': true,
            'attachment': {
              'dataUrl':
                  'data:image/png;base64,${base64Encode(pngFixture(width: 32, height: 16))}',
            },
          });
        }
        return mermaidResponse(imageEnvelope());
      },
    );
    await settleMermaid(t);
    expect(find.byType(ChatAttachmentTile), findsOneWidget);
    expect(find.bySemanticsLabel('shot.png'), findsWidgets);
    expect(ready(), findsOneWidget);
    expect(
      requests
          .singleWhere((r) => r.url.path.contains('/attachments/'))
          .headers['authorization'],
      startsWith('Bearer '),
    );
    expect(t.takeException(), isNull);
  });
  testWidgets('확대 화면도 PC 변경 동안 이전 준비 이미지를 숨긴다', (t) async {
    final pending = Completer<http.Response>();
    final c = await mountMermaid(
      t,
      const MermaidDiagram(definition: definition),
      (r) async {
        final width = (jsonDecode(r.body) as Map)['width'] as int;
        if (r.url.host == 'other' && width == 800) return pending.future;
        return mermaidResponse(imageEnvelope(width: width));
      },
    );
    await settleMermaid(t);
    await t.tap(find.byType(InkWell).first);
    await settleMermaid(t);
    Finder zoomReady() => find.byWidgetPredicate(
      (w) => w is Semantics && w.properties.identifier == 'mermaid-zoom-ready',
    );
    expect(zoomReady(), findsOneWidget);
    await c.read(connectionControllerProvider.notifier).select(otherPc.id);
    await t.pump();
    await t.pump(const Duration(milliseconds: 100));
    expect(zoomReady(), findsNothing);
    pending.complete(mermaidResponse(imageEnvelope(width: 800, height: 220)));
    await settleMermaid(t);
    expect(zoomReady(), findsOneWidget);
    final visible = t.widget<Image>(find.byType(Image));
    expect(
      ByteData.sublistView((visible.image as MemoryImage).bytes).getUint32(20),
      220,
    );
  });

  testWidgets('loading과 raw source는 ready가 아니며 PNG frame이 준비돼야 성공한다', (
    t,
  ) async {
    final pending = Completer<http.Response>();
    await mountMermaid(
      t,
      const MermaidDiagram(definition: definition),
      (_) async => pending.future,
    );
    expect(find.byType(CircularProgressIndicator), findsOneWidget);
    expect(ready(), findsNothing);
    expect(find.textContaining('Raw source only'), findsNothing);
    pending.complete(mermaidResponse(imageEnvelope()));
    await settleMermaid(t);
    expect(find.byType(Image), findsOneWidget);
    expect(ready(), findsOneWidget);
    expect(find.textContaining('Raw source only'), findsNothing);
  });
  testWidgets('정상 PNG에 확대·원문보기·복사를 연결한다', (t) async {
    String? copied;
    t.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      (call) async {
        if (call.method == 'Clipboard.setData') {
          copied = (call.arguments as Map)['text'] as String;
        }
        return null;
      },
    );
    addTearDown(
      () => t.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        null,
      ),
    );
    await mountMermaid(
      t,
      const MermaidDiagram(definition: definition),
      (r) async => mermaidResponse(
        imageEnvelope(width: (jsonDecode(r.body) as Map)['width'] as int),
      ),
    );
    await settleMermaid(t);
    expect(ready(), findsOneWidget);
    await t.tap(find.text('원문 복사'));
    await t.pump();
    expect(copied, definition);
    await t.tap(find.byType(InkWell).first);
    await settleMermaid(t);
    expect(find.byType(InteractiveViewer), findsOneWidget);
    expect(
      find.byWidgetPredicate(
        (w) =>
            w is Semantics && w.properties.identifier == 'mermaid-zoom-ready',
      ),
      findsOneWidget,
    );
    await t.pageBack();
    await settleMermaid(t);
    await t.tap(find.text('원문 보기'));
    await settleMermaid(t);
    expect(find.text(definition), findsOneWidget);
  });
  testWidgets('서버 오류에서 재시도하면 새 PNG를 기다린다', (t) async {
    var fail = true;
    await mountMermaid(
      t,
      const MermaidDiagram(definition: definition),
      (_) async => fail
          ? mermaidResponse({
              'success': false,
              'error': 'unavailable',
            }, status: 503)
          : mermaidResponse(imageEnvelope()),
    );
    await settleMermaid(t);
    expect(find.text('다이어그램을 표시하지 못했습니다.'), findsOneWidget);
    expect(ready(), findsNothing);
    fail = false;
    await t.tap(find.text('다시 시도'));
    await settleMermaid(t);
    expect(ready(), findsOneWidget);
  });
  testWidgets('IHDR는 유효해도 PNG decode 실패는 ready가 아니며 재시도할 수 있다', (t) async {
    var malformed = true;
    final bytes = Uint8List.fromList(pngFixture());
    bytes[45] ^= 255;
    await mountMermaid(
      t,
      const MermaidDiagram(definition: definition),
      (_) async =>
          mermaidResponse(imageEnvelope(bytes: malformed ? bytes : null)),
    );
    await settleMermaid(t);
    expect(ready(), findsNothing);
    expect(
      find.text('다이어그램 이미지를 읽지 못했습니다. 원문을 확인하거나 다시 시도해 주세요.'),
      findsOneWidget,
    );
    expect(find.textContaining('Raw source only'), findsNothing);
    malformed = false;
    await t.tap(find.text('다시 시도'));
    await settleMermaid(t);
    expect(ready(), findsOneWidget);
  });
  testWidgets('정의와 theme 변경 때 이전 ready를 숨기고 새 요청을 보낸다', (t) async {
    final requests = <Map<String, dynamic>>[];
    final pending = Completer<http.Response>();
    final c = await mountMermaid(
      t,
      const MermaidDiagram(definition: definition),
      (r) async {
        requests.add(jsonDecode(r.body) as Map<String, dynamic>);
        if (requests.length > 1) return pending.future;
        return mermaidResponse(imageEnvelope());
      },
    );
    await settleMermaid(t);
    expect(ready(), findsOneWidget);
    await t.pumpWidget(
      UncontrolledProviderScope(
        container: c,
        child: MaterialApp(
          theme: buildAppTheme(Brightness.light),
          home: const Scaffold(
            body: Center(
              child: SizedBox(
                width: 400,
                child: MermaidDiagram(definition: 'graph TD\nX --> Y'),
              ),
            ),
          ),
        ),
      ),
    );
    await t.pump();
    await t.pump(const Duration(milliseconds: 100));
    expect(ready(), findsNothing);
    expect(requests.last, {
      'definition': 'graph TD\nX --> Y',
      'theme': 'light',
      'width': 400,
    });
    pending.complete(mermaidResponse(imageEnvelope()));
    await settleMermaid(t);
    expect(ready(), findsOneWidget);
  });
}
