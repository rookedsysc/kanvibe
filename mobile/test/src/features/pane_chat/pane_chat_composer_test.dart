import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/chat_history/domain/chat_history.dart';
import 'package:kanvibe_mobile/src/features/chat_history/presentation/chat_message_tile.dart';
import 'package:kanvibe_mobile/src/features/pane_chat/application/pane_chat_composer_controller.dart';
import 'package:kanvibe_mobile/src/features/pane_chat/application/pane_chat_controller.dart';
import 'package:kanvibe_mobile/src/features/pane_chat/domain/pane_chat_attachment.dart';

import 'package:kanvibe_mobile/src/features/pane_chat/presentation/pane_chat_attachment_strip.dart';
import 'package:kanvibe_mobile/src/features/pane_chat/presentation/pane_chat_view.dart';
import 'package:kanvibe_mobile/src/features/pane_chat/data/pane_chat_attachment_intake.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';

import '../../../support/fake_desktop.dart';

import 'pane_chat_test.dart' as fixtures;

const owner = (
  connectionId: 'a:19746',
  taskId: 'task/1',
  paneId: '%19',
  conversationKey: 'target-a',
);
final image = PickedPaneChatAttachment(
  name: '사진.png',
  mimeType: 'image/png',
  bytes: Uint8List.fromList([1, 2, 3]),
);
const imageData =
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=';

Map<String, dynamic> snapshot({String token = 'target-a'}) =>
    fixtures.snapshotJson(token: token)
      ..['composer'] = {
        'attachments': true,
        'skills': true,
        'fileMentions': true,
        'maxAttachmentBytes': 10 * 1024 * 1024,
        'maxAttachments': 10,
      };

Future<({ProviderContainer container, PaneChatComposerController controller})>
ready(Future<http.Response> Function(http.Request) handler) async {
  final client = DesktopClient(httpClient: MockClient(handler));
  final container = fixtures.setup(client);
  addTearDown(container.dispose);
  addTearDown(client.close);
  final paneSub = container.listen(
    paneChatProvider(fixtures.target),
    (_, _) {},
  );
  addTearDown(paneSub.close);
  await container.read(paneChatProvider(fixtures.target).future);
  final sub = container.listen(paneChatComposerProvider(owner), (_, _) {});
  addTearDown(sub.close);
  final controller = container.read(paneChatComposerProvider(owner).notifier);
  await controller.setActive(true);
  return (container: container, controller: controller);
}

http.Response ok() => fixtures.response({'success': true});
http.Response staged(String id) => fixtures.response({
  'success': true,
  'attachment': {
    'id': id,
    'name': '사진.png',
    'mimeType': 'image/png',
    'kind': 'image',
    'sizeBytes': 3,
  },
});
Future<void> flush() async {
  for (var i = 0; i < 12; i++) {
    await Future<void>.delayed(Duration.zero);
  }
}

void main() {
  test('uncertain send keeps operation ID across lease renewal and attachment restaging', () async {
    var token = 'target-a', serial = 0;
    final sends = <Map<String, dynamic>>[];
    final env = await ready((request) async {
      if (request.method == 'GET') {
        return fixtures.response({
          'snapshot': snapshot(token: token)..['conversationKey'] = 'target-a',
        });
      }
      if (request.method == 'DELETE') return ok();
      if (request.url.pathSegments.last == 'attachments') {
        return staged('staged-${++serial}');
      }
      sends.add(jsonDecode(request.body) as Map<String, dynamic>);
      return fixtures.response({'error': 'delivery unknown'}, 503);
    });
    env.controller.edit('same operation');
    env.controller.addAttachments([image]);
    await flush();
    expect(await env.controller.send(), isFalse);
    token = 'renewed-X';
    await env.container
        .read(paneChatProvider(fixtures.target).notifier)
        .refresh();
    await env.controller.setActive(true);
    await flush();
    expect(await env.controller.send(), isFalse);
    expect(sends.length, 2);
    expect(sends[0]['requestId'], isNotNull);
    expect(sends[1]['requestId'], sends[0]['requestId']);
    expect(sends[1]['targetToken'], 'renewed-X');
    expect(sends[0]['attachmentIds'], ['staged-1']);
    expect(sends[1]['attachmentIds'], ['staged-2']);
  });

  TestWidgetsFlutterBinding.ensureInitialized();
  setUp(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
          const MethodChannel('kanvibe/pane-chat-attachments'),
          (_) async => null,
        );
  });
  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
          const MethodChannel('kanvibe/pane-chat-attachments'),
          null,
        );
  });

  test('authenticated attachment/capability/catalog/media HTTP payloads and encoded segments', () async {
    final requests = <http.Request>[];
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        requests.add(request);
        if (request.url.pathSegments.contains('ai-sessions')) {
          return fixtures.response({
            'success': true,
            'attachment': {
              'id': 'media/1',
              'name': '사진.png',
              'mimeType': 'image/png',
              'dataUrl': 'data:image/png;base64,$imageData',
            },
          });
        }
        if (request.url.pathSegments.last == 'suggestions') {
          return fixtures.response({
            'success': true,
            'targetToken': 'lease',
            'items': [
              {
                'id': 's',
                'label': '스킬',
                'description': '설명',
                'insertText': r'$skill',
              },
            ],
            'truncated': false,
          });
        }
        if (request.method == 'POST' &&
            request.url.pathSegments.last == 'attachments') {
          return staged('id');
        }
        return ok();
      }),
    );
    addTearDown(client.close);
    expect(
      await client.stagePaneChatAttachment(
        fixtures.pcA,
        'task/1',
        'lease',
        name: image.name,
        mimeType: image.mimeType,
        bytes: image.bytes,
      ),
      'id',
    );
    await client.removePaneChatAttachment(
      fixtures.pcA,
      'task/1',
      'lease',
      'id',
    );
    expect(
      (await client.getPaneChatSuggestions(
        fixtures.pcA,
        'task/1',
        'lease',
        'skill',
        '스킬',
      )).items.single.insertText,
      r'$skill',
    );
    expect(
      await client.fetchChatAttachment(
        fixtures.pcA,
        'task/1',
        'codex',
        'session/1',
        'media/1',
      ),
      base64Decode(imageData),
    );
    expect(
      requests.every((r) => r.headers['authorization'] == 'Bearer token-a'),
      true,
    );
    expect(requests.first.url.pathSegments[3], 'task/1');
    expect(jsonDecode(requests.first.body), {
      'targetToken': 'lease',
      'attachment': {
        'name': '사진.png',
        'mimeType': 'image/png',
        'dataUrl': 'data:image/png;base64,AQID',
      },
    });
    expect(jsonDecode(requests[1].body), {'targetToken': 'lease', 'id': 'id'});
    expect(requests[2].url.queryParameters, {
      'targetToken': 'lease',
      'kind': 'skill',
      'query': '스킬',
    });
    expect(snapshot()['composer']['attachments'], true);
  });

  test('upload failure retains bytes, retry stages and image-only send clears accepted IDs', () async {
    var uploads = 0;
    final sends = <Map>[];
    final env = await ready((r) async {
      if (r.url.pathSegments.last == 'attachments') {
        return ++uploads == 1
            ? fixtures.response({'error': 'offline'}, 503)
            : staged('ready-id');
      }
      if (r.method == 'GET') return fixtures.response({'snapshot': snapshot()});
      sends.add(jsonDecode(r.body) as Map);
      return ok();
    });
    env.controller.addAttachments([image]);
    await flush();
    var item = env.container
        .read(paneChatComposerProvider(owner))
        .attachments
        .single;
    expect(item.error, isNotNull);
    expect(item.file.bytes, [1, 2, 3]);
    expect(await env.controller.send(), false);
    await env.controller.upload(item.localId);
    expect(env.container.read(paneChatComposerProvider(owner)).canSend, true);
    expect(await env.controller.send(), true);
    expect(sends.single['text'], '');
    expect(sends.single['attachmentIds'], ['ready-id']);
    expect(sends.single['requestId'], isNotEmpty);
    expect(
      env.container.read(paneChatComposerProvider(owner)).attachments,
      isEmpty,
    );
  });

  test(
    'send retry reuses request ID and ready attachment without a second upload',
    () async {
      var uploads = 0;
      final sends = <Map>[];
      final env = await ready((r) async {
        if (r.url.pathSegments.last == 'attachments') {
          uploads++;
          return staged('ready');
        }
        if (r.method == 'GET') {
          return fixtures.response({'snapshot': snapshot()});
        }
        sends.add(jsonDecode(r.body) as Map);
        return sends.length == 1
            ? fixtures.response({'error': 'offline'}, 503)
            : ok();
      });
      env.controller.edit('caption');
      env.controller.addAttachments([image]);
      await flush();
      expect(await env.controller.send(), false);
      expect(
        env.container.read(paneChatComposerProvider(owner)).text,
        'caption',
      );
      expect(await env.controller.send(), true);
      expect(uploads, 1);
      expect(sends[0]['requestId'], sends[1]['requestId']);
      expect(sends[0]['attachmentIds'], sends[1]['attachmentIds']);
    },
  );

  test('ACK preserves edits and newly added attachment during send; only captured IDs clear', () async {
    final send = Completer<http.Response>();
    var uploads = 0;
    final env = await ready((r) async {
      if (r.url.pathSegments.last == 'attachments') {
        return staged('id-${++uploads}');
      }
      if (r.method == 'GET') return fixtures.response({'snapshot': snapshot()});
      return send.future;
    });
    env.controller.edit('first');
    env.controller.addAttachments([image]);
    await flush();
    final pending = env.controller.send();
    await flush();
    env.controller.edit('new text');
    env.controller.addAttachments([image]);
    await flush();
    send.complete(ok());
    expect(await pending, true);
    final state = env.container.read(paneChatComposerProvider(owner));
    expect(state.text, 'new text');
    expect(state.attachments.single.id, 'id-2');
  });

  test('removed inflight upload is deleted on completion and never resurrects draft', () async {
    final upload = Completer<http.Response>();
    final deletes = <String>[];
    final env = await ready((r) async {
      if (r.method == 'DELETE') {
        deletes.add((jsonDecode(r.body) as Map)['id']);
        return ok();
      }
      if (r.method == 'GET') return fixtures.response({'snapshot': snapshot()});
      return upload.future;
    });
    env.controller.addAttachments([image]);
    await flush();
    await env.controller.remove(
      env.container
          .read(paneChatComposerProvider(owner))
          .attachments
          .single
          .localId,
    );
    upload.complete(staged('removed'));
    await flush();
    expect(deletes, ['removed']);
    expect(
      env.container.read(paneChatComposerProvider(owner)).attachments,
      isEmpty,
    );
  });

  test('target token / PC switch blocks old composer input and isolates next owner', () async {
    var token = 'target-a', writes = 0;
    final env = await ready((r) async {
      if (r.method == 'GET') {
        return fixtures.response({'snapshot': snapshot(token: token)});
      }
      writes++;
      return ok();
    });
    env.controller.edit('private A');
    token = 'target-b';
    await env.container
        .read(paneChatProvider(fixtures.target).notifier)
        .refresh();
    expect(await env.controller.send(), false);
    const next = (
      connectionId: 'a:19746',
      taskId: 'task/1',
      paneId: '%19',
      conversationKey: 'target-b',
    );
    final sub = env.container.listen(paneChatComposerProvider(next), (_, _) {});
    addTearDown(sub.close);
    expect(env.container.read(paneChatComposerProvider(next)).text, '');
    await env.container
        .read(connectionControllerProvider.notifier)
        .select(fixtures.pcB.id);
    expect(await env.controller.send(), false);
    expect(writes, 0);
  });

  test(
    '10-count and decoded-size bounds reject atomically before HTTP staging',
    () async {
      var writes = 0;
      final env = await ready((r) async {
        if (r.method == 'GET') {
          return fixtures.response({'snapshot': snapshot()});
        }
        writes++;
        return staged('id');
      });
      env.controller.addAttachments(List.filled(11, image));
      expect(
        env.container.read(paneChatComposerProvider(owner)).attachments,
        isEmpty,
      );
      env.controller.addAttachments([
        PickedPaneChatAttachment(
          name: 'big',
          mimeType: 'text/plain',
          bytes: Uint8List(10 * 1024 * 1024 + 1),
        ),
      ]);
      expect(
        env.container.read(paneChatComposerProvider(owner)).attachments,
        isEmpty,
      );
      expect(writes, 0);
    },
  );

  test('unsupported HEIC image is a visible format error and never a generic file upload', () async {
    var writes = 0;
    final env = await ready((r) async {
      if (r.method == 'GET') {
        return fixtures.response({'snapshot': snapshot()});
      }
      writes++;
      return staged('id');
    });
    env.controller.addAttachments([
      PickedPaneChatAttachment(
        name: 'photo.heic',
        mimeType: 'image/heic',
        bytes: Uint8List.fromList([1]),
      ),
    ]);
    expect(
      env.container.read(paneChatComposerProvider(owner)).error,
      contains('지원하지 않습니다'),
    );
    expect(
      env.container.read(paneChatComposerProvider(owner)).attachments,
      isEmpty,
    );
    expect(writes, 0);
  });

  test('catalog dismiss/target change suppresses late responses and error is recoverable', () async {
    final late = Completer<http.Response>();
    var calls = 0;
    final env = await ready((r) async {
      if (r.url.pathSegments.last == 'suggestions') {
        calls++;
        return calls == 1
            ? late.future
            : fixtures.response({
                'success': true,
                'targetToken': 'target-a',
                'items': [
                  {
                    'id': 'b',
                    'label': 'new',
                    'description': '',
                    'insertText': r'$new',
                  },
                ],
              });
      }
      return fixtures.response({'snapshot': snapshot()});
    });
    env.controller.suggest('skill', 'old');
    await Future<void>.delayed(const Duration(milliseconds: 220));
    env.controller.dismissSuggestions();
    late.complete(
      fixtures.response({
        'success': true,
        'targetToken': 'target-a',
        'items': [
          {'id': 'a', 'label': 'old', 'insertText': r'$old'},
        ],
      }),
    );
    await flush();
    expect(
      env.container.read(paneChatComposerProvider(owner)).suggestions,
      isEmpty,
    );
    env.controller.suggest('skill', 'new');
    await Future<void>.delayed(const Duration(milliseconds: 220));
    expect(
      env.container
          .read(paneChatComposerProvider(owner))
          .suggestions
          .single
          .label,
      'new',
    );
  });

  group('skill·명령 목록', () {
    Map<String, String> suggestion(String label) => {
      'id': label,
      'label': label,
      'description': '',
      'insertText': '/$label',
    };
    http.Response catalog(List<String> labels, {bool truncated = false}) =>
        fixtures.response({
          'success': true,
          'targetToken': 'target-a',
          'items': labels.map(suggestion).toList(),
          'truncated': truncated,
        });
    List<String> shownLabels(ProviderContainer container) => container
        .read(paneChatComposerProvider(owner))
        .suggestions
        .map((item) => item.label)
        .toList();

    test('메뉴를 열 때 한 번만 받고 이어지는 글자 입력은 기기에서 바로 거른다', () async {
      final requests = <Map<String, String>>[];
      final env = await ready((r) async {
        if (r.url.pathSegments.last == 'suggestions') {
          requests.add(r.url.queryParameters);
          return catalog(['plan', 'review', 'test']);
        }
        return fixtures.response({'snapshot': snapshot()});
      });
      env.controller.suggest('command', '');
      await flush();
      expect(shownLabels(env.container), ['plan', 'review', 'test']);
      expect(requests.single, {
        'targetToken': 'target-a',
        'kind': 'command',
        'query': '',
        'limit': '512',
      });

      env.controller.suggest('command', 're');
      expect(shownLabels(env.container), ['review']);
      expect(
        env.container
            .read(paneChatComposerProvider(owner))
            .isLoadingSuggestions,
        false,
      );
      env.controller.suggest('command', 'zzz');
      expect(shownLabels(env.container), isEmpty);
      await Future<void>.delayed(const Duration(milliseconds: 220));
      expect(requests, hasLength(1));
    });

    test('일부만 받은 목록이면 글자마다 데스크톱에 묻는다', () async {
      final requests = <Map<String, String>>[];
      final env = await ready((r) async {
        if (r.url.pathSegments.last == 'suggestions') {
          requests.add(r.url.queryParameters);
          final query = r.url.queryParameters['query']!;
          return r.url.queryParameters.containsKey('limit')
              ? catalog(['alpha'], truncated: true)
              : catalog(['found-$query']);
        }
        return fixtures.response({'snapshot': snapshot()});
      });
      env.controller.suggest('skill', '');
      await flush();
      await Future<void>.delayed(const Duration(milliseconds: 220));
      expect(shownLabels(env.container), ['found-']);

      env.controller.suggest('skill', 'rev');
      await Future<void>.delayed(const Duration(milliseconds: 220));
      expect(shownLabels(env.container), ['found-rev']);
      expect(requests.last, {
        'targetToken': 'target-a',
        'kind': 'skill',
        'query': 'rev',
      });
    });

    test('메뉴를 다시 열면 받아 둔 목록을 바로 보여 주고 뒤에서 새로 받는다', () async {
      final refreshed = Completer<http.Response>();
      var calls = 0;
      final env = await ready((r) async {
        if (r.url.pathSegments.last == 'suggestions') {
          calls++;
          return calls == 1 ? catalog(['review']) : refreshed.future;
        }
        return fixtures.response({'snapshot': snapshot()});
      });
      env.controller.suggest('skill', '');
      await flush();
      env.controller.dismissSuggestions();

      env.controller.suggest('skill', '');
      expect(shownLabels(env.container), ['review']);
      await flush();
      expect(calls, 2);
      refreshed.complete(catalog(['plan', 'review']));
      await flush();
      expect(shownLabels(env.container), ['plan', 'review']);
    });

    test(
      '다시 받기에 실패하면 받아 둔 목록을 유지하고, 처음 받기에 실패하면 오류를 보인 뒤 다음 입력에서 다시 받는다',
      () async {
        var calls = 0;
        final env = await ready((r) async {
          if (r.url.pathSegments.last == 'suggestions') {
            calls++;
            if (r.url.queryParameters['kind'] == 'skill') {
              return calls == 1
                  ? catalog(['review'])
                  : http.Response('{"success":false}', 500);
            }
            return calls == 3
                ? http.Response('{"success":false}', 500)
                : catalog(['help']);
          }
          return fixtures.response({'snapshot': snapshot()});
        });
        env.controller.suggest('skill', '');
        await flush();
        env.controller.dismissSuggestions();
        env.controller.suggest('skill', '');
        await flush();
        expect(calls, 2);
        expect(shownLabels(env.container), ['review']);
        expect(
          env.container.read(paneChatComposerProvider(owner)).suggestionError,
          isNull,
        );

        env.controller.suggest('command', '');
        await flush();
        expect(
          env.container.read(paneChatComposerProvider(owner)).suggestionError,
          isNotNull,
        );
        env.controller.suggest('command', 'h');
        await flush();
        expect(shownLabels(env.container), ['help']);
      },
    );
  });

  test(
    'native intake failure exposes human message without platform code',
    () async {
      final env = await ready(
        (r) async => fixtures.response({'snapshot': snapshot()}),
      );
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(
            const MethodChannel('kanvibe/pane-chat-attachments'),
            (call) async {
              if (call.method == 'readClipboardImage') {
                throw PlatformException(
                  code: 'attachment_read_failed',
                  message: '파일을 읽지 못했습니다.',
                );
              }
              return null;
            },
          );
      await env.controller.pasteImage();
      expect(
        env.container.read(paneChatComposerProvider(owner)).error,
        '파일을 읽지 못했습니다.',
      );
    },
  );

  testWidgets('upload retry accepts the entire attachment card touch target', (
    tester,
  ) async {
    String? retried;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: PaneChatAttachmentStrip(
            attachments: [
              PaneChatDraftAttachment(
                localId: 'a',
                file: image,
                error: '업로드 실패 · 다시 시도',
              ),
            ],
            sending: false,
            onRemove: (_) {},
            onRetry: (id) => retried = id,
          ),
        ),
      ),
    );
    final card = find.byType(Card);
    expect(tester.getSize(card).shortestSide, greaterThanOrEqualTo(48));
    await tester.tapAt(tester.getCenter(card));
    await tester.pump();
    expect(retried, 'a');
  });

  test('dispose clears all 10 ready unsent IDs so reopening same CLI lease can stage again', () async {
    final stagedIds = <String>{};
    final deleted = <String>[];
    var serial = 0;
    final env = await ready((r) async {
      if (r.method == 'DELETE') {
        final body = jsonDecode(r.body) as Map;
        expect(body['targetToken'], 'target-a');
        expect(r.url.pathSegments[3], 'task/1');
        deleted.add(body['id']);
        stagedIds.remove(body['id']);
        return ok();
      }
      if (r.method == 'GET') {
        return fixtures.response({'snapshot': snapshot()});
      }
      if (stagedIds.length == 10) {
        return fixtures.response({'error': 'quota'}, 409);
      }
      final id = 'draft-${++serial}';
      stagedIds.add(id);
      return staged(id);
    });
    env.controller.addAttachments(List.filled(10, image));
    await flush();
    expect(stagedIds.length, 10);
    env.container.invalidate(paneChatComposerProvider(owner));
    await flush();
    expect(
      deleted.toSet(),
      Set.from(List.generate(10, (i) => 'draft-${i + 1}')),
    );
    expect(stagedIds, isEmpty);
    final reopened = env.container.read(
      paneChatComposerProvider(owner).notifier,
    );
    await reopened.setActive(true);
    reopened.addAttachments([image]);
    await flush();
    expect(
      env.container.read(paneChatComposerProvider(owner)).attachments.single.id,
      'draft-11',
    );
  });

  test('late upload ACK after disposal is cleaned with captured client and original connection', () async {
    final upload = Completer<http.Response>();
    final deleted = <String>[];
    final env = await ready((r) async {
      if (r.method == 'DELETE') {
        deleted.add((jsonDecode(r.body) as Map)['id']);
        return ok();
      }
      if (r.method == 'GET') {
        return fixtures.response({'snapshot': snapshot()});
      }
      return upload.future;
    });
    env.controller.addAttachments([image]);
    await flush();
    env.container.invalidate(paneChatComposerProvider(owner));
    await flush();
    upload.complete(staged('late-original'));
    await flush();
    expect(deleted, ['late-original']);
    expect(
      env.container.read(paneChatComposerProvider(owner)).attachments,
      isEmpty,
    );
  });

  for (final accepted in [false, true]) {
    test(
      'dispose defers pending-send ID cleanup until outcome accepted=$accepted',
      () async {
        final send = Completer<http.Response>();
        final deleted = <String>[];
        var retained = false, exists = false;
        final env = await ready((r) async {
          if (r.method == 'DELETE') {
            deleted.add((jsonDecode(r.body) as Map)['id']);
            if (!retained) exists = false;
            return ok();
          }
          if (r.method == 'GET') {
            return fixtures.response({'snapshot': snapshot()});
          }
          if (r.url.pathSegments.last == 'attachments') {
            exists = true;
            return staged('pending-file');
          }
          return send.future;
        });
        env.controller.addAttachments([image]);
        await flush();
        final pending = env.controller.send();
        await flush();
        env.container.invalidate(paneChatComposerProvider(owner));
        await flush();
        expect(deleted, isEmpty);
        expect(exists, true);
        retained = accepted;
        send.complete(
          accepted
              ? ok()
              : fixtures.response({'error': 'prewrite failure'}, 503),
        );
        await pending;
        await flush();
        expect(deleted, ['pending-file']);
        expect(exists, accepted);
      },
    );
  }

  test('hidden pane deactivation retains ready IDs and draft until actual disposal', () async {
    final deleted = <String>[];
    final env = await ready((r) async {
      if (r.method == 'DELETE') {
        deleted.add((jsonDecode(r.body) as Map)['id']);
        return ok();
      }
      if (r.method == 'GET') {
        return fixtures.response({'snapshot': snapshot()});
      }
      return staged('retained-hidden');
    });
    env.controller.edit('hidden draft');
    env.controller.addAttachments([image]);
    await flush();
    await env.controller.setActive(false);
    await flush();
    expect(deleted, isEmpty);
    expect(
      env.container.read(paneChatComposerProvider(owner)).text,
      'hidden draft',
    );
    await env.controller.setActive(true);
    expect(
      env.container.read(paneChatComposerProvider(owner)).attachments.single.id,
      'retained-hidden',
    );
  });

  testWidgets(
    'attachment sheet reactivates route owner before opening native picker',
    (tester) async {
      var picks = 0;
      final client = DesktopClient(
        httpClient: MockClient(
          (r) async => fixtures.response({'snapshot': snapshot()}),
        ),
      );
      final intake = PaneChatAttachmentIntake(
        pickFiles: (_) async {
          picks++;
          return [];
        },
      );
      final container = ProviderContainer(
        overrides: [
          desktopClientProvider.overrideWithValue(client),
          connectionStoreProvider.overrideWithValue(
            FakeConnectionStore(
              initial: const SavedConnections(
                connections: [fixtures.pcA],
                activeId: 'a:19746',
              ),
            ),
          ),
          paneChatAttachmentIntakeProvider.overrideWithValue(intake),
        ],
      );
      addTearDown(container.dispose);
      addTearDown(client.close);
      addTearDown(intake.dispose);
      await tester.pumpWidget(
        UncontrolledProviderScope(
          container: container,
          child: MaterialApp(
            home: Builder(
              builder: (context) => Scaffold(
                body: PaneChatView(
                  target: fixtures.target,
                  onReturnToTerminal: () {},
                  isActive: ModalRoute.isCurrentOf(context) ?? true,
                ),
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byTooltip('첨부 추가'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('사진 선택'));
      await tester.pumpAndSettle();
      expect(picks, 1);
      await tester.pumpWidget(const SizedBox.shrink());
      container.dispose();
      await tester.pump();
    },
  );

  testWidgets(
    'skill button opens catalog and inserts provider-owned slash syntax',
    (tester) async {
      final client = DesktopClient(
        httpClient: MockClient((r) async {
          if (r.url.pathSegments.last == 'suggestions') {
            expect(r.url.queryParameters['kind'], 'skill');
            expect(r.url.queryParameters['query'], '');
            return fixtures.response({
              'success': true,
              'targetToken': 'target-a',
              'items': [
                {
                  'id': 'skill',
                  'label': 'probe skill',
                  'description': '',
                  'insertText': '/provider-probe',
                },
              ],
            });
          }
          return fixtures.response({'snapshot': snapshot()});
        }),
      );
      final container = fixtures.setup(client);
      addTearDown(container.dispose);
      addTearDown(client.close);
      await fixtures.pumpChat(tester, container);
      await tester.tap(find.byTooltip('스킬 선택'));
      await tester.pump(const Duration(milliseconds: 250));
      await tester.pumpAndSettle();
      await tester.tap(find.text('probe skill'));
      await tester.pump();
      expect(
        tester.widget<TextField>(find.byType(TextField)).controller!.text,
        '/provider-probe ',
      );
      await tester.pumpWidget(const SizedBox.shrink());
      container.dispose();
      await tester.pump();
    },
  );

  testWidgets(
    'completion touch insertion replaces token, preserves Korean prefix and trailing text',
    (tester) async {
      final client = DesktopClient(
        httpClient: MockClient((r) async {
          if (r.url.pathSegments.last == 'suggestions') {
            return fixtures.response({
              'success': true,
              'targetToken': 'target-a',
              'items': [
                {
                  'id': '1',
                  'label': 'qa skill',
                  'description': '설명',
                  'insertText': r'$qa-probe',
                },
              ],
            });
          }
          return fixtures.response({'snapshot': snapshot()});
        }),
      );
      final container = fixtures.setup(client);
      addTearDown(container.dispose);
      addTearDown(client.close);
      await fixtures.pumpChat(tester, container);
      await tester.enterText(find.byType(TextField), r'한글 $qa');
      await tester.pump(const Duration(milliseconds: 250));
      await tester.pumpAndSettle();
      await tester.tap(find.text('qa skill'));
      await tester.pump();
      expect(
        tester.widget<TextField>(find.byType(TextField)).controller!.text,
        r'한글 $qa-probe ',
      );
      await tester.pumpWidget(const SizedBox.shrink());
      container.dispose();
      await tester.pump();
    },
  );

  testWidgets(
    'completion Enter/Tab/Escape and IME composing never accidentally submit',
    (tester) async {
      var sends = 0;
      final client = DesktopClient(
        httpClient: MockClient((r) async {
          if (r.url.pathSegments.last == 'suggestions') {
            return fixtures.response({
              'success': true,
              'targetToken': 'target-a',
              'items': [
                {'id': '1', 'label': 'alpha', 'insertText': r'$alpha'},
                {'id': '2', 'label': 'beta', 'insertText': r'$beta'},
              ],
            });
          }
          if (r.method != 'GET') {
            sends++;
            return ok();
          }
          return fixtures.response({'snapshot': snapshot()});
        }),
      );
      final container = fixtures.setup(client);
      addTearDown(container.dispose);
      addTearDown(client.close);
      await fixtures.pumpChat(tester, container);
      await tester.enterText(find.byType(TextField), r'$a');
      await tester.pump(const Duration(milliseconds: 250));
      await tester.pumpAndSettle();
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await tester.pump();
      expect(
        tester.widget<TextField>(find.byType(TextField)).controller!.text,
        r'$beta ',
      );
      expect(sends, 0);
      await tester.enterText(find.byType(TextField), r'$a');
      await tester.pump(const Duration(milliseconds: 250));
      await tester.pumpAndSettle();
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pump();
      expect(find.text('alpha'), findsNothing);
      final input = tester
          .widget<TextField>(find.byType(TextField))
          .controller!;
      input.value = const TextEditingValue(
        text: r'$a',
        selection: TextSelection.collapsed(offset: 2),
        composing: TextRange(start: 1, end: 2),
      );
      await tester.pump(const Duration(milliseconds: 250));
      expect(find.text('alpha'), findsNothing);
      expect(sends, 0);
      await tester.pumpWidget(const SizedBox.shrink());
      container.dispose();
      await tester.pump();
    },
  );

  testWidgets(
    'image-only history renders authenticated bytes without empty text, error retries',
    (tester) async {
      var mediaReads = 0;
      final client = DesktopClient(
        httpClient: MockClient((r) async {
          mediaReads++;
          expect(r.headers['authorization'], 'Bearer token-a');
          return mediaReads == 1
              ? fixtures.response({'error': 'gone'}, 404)
              : fixtures.response({
                  'success': true,
                  'attachment': {
                    'id': 'img',
                    'name': 'photo.png',
                    'mimeType': 'image/png',
                    'dataUrl': 'data:image/png;base64,$imageData',
                  },
                });
        }),
      );
      final container = fixtures.setup(client);
      addTearDown(container.dispose);
      addTearDown(client.close);
      final message = ChatMessage.fromJson({
        'role': 'user',
        'fullText': '',
        'attachments': [
          {
            'id': 'img',
            'kind': 'image',
            'name': 'photo.png',
            'mimeType': 'image/png',
            'sizeBytes': 68,
          },
        ],
      });
      await tester.pumpWidget(
        UncontrolledProviderScope(
          container: container,
          child: MaterialApp(
            home: Scaffold(
              body: ChatMessageTile(
                message: message,
                provider: 'codex',
                taskId: 'task/1',
                sessionId: 'ended-session',
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.textContaining('불러오지 못했습니다'), findsOneWidget);
      await tester.tap(find.byTooltip('이미지 다시 불러오기'));
      await tester.pumpAndSettle();
      expect(find.byType(Image), findsOneWidget);
      expect(mediaReads, 2);
      expect(find.byType(SelectableText), findsNothing);
    },
  );
}
