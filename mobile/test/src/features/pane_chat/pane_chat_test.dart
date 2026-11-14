import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/constants/app_theme.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/board/domain/board_task.dart';
import 'package:kanvibe_mobile/src/features/board/domain/task_status.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/pane_chat/application/pane_chat_controller.dart';
import 'package:kanvibe_mobile/src/features/pane_chat/domain/pane_chat.dart';
import 'package:kanvibe_mobile/src/features/pane_chat/presentation/pane_chat_view.dart';
import 'package:kanvibe_mobile/src/features/task_detail/domain/mirror_pane.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/task_detail_screen.dart';

import '../../../support/fake_desktop.dart';

const pcA = DesktopConnection(host: 'a', port: 19746, token: 'token-a');
const pcB = DesktopConnection(host: 'b', port: 19746, token: 'token-b');
const target = (taskId: 'task/1', paneId: '%19');

Map<String, dynamic> snapshotJson({
  PaneChatTarget target = target,
  String token = 'target-a',
  String? provider = 'codex',
  String inputState = 'ready',
  bool available = true,
  String screen = '› 현재 CLI 화면',
}) => {
  'taskId': target.taskId,
  'paneId': target.paneId,
  'sessionType': 'tmux',
  'provider': provider,
  'targetToken': provider == null ? null : token,
  'canSend': provider != null,
  'inputState': inputState,
  'screen': screen,
  'reason': provider == null ? 'no-foreground-ai-cli' : null,
  'transcript': {
    'status': available ? 'available' : 'unavailable',
    'reason': available ? null : 'exact-session-unavailable',
    'sessionId': available ? 'exact-session' : null,
    'messages': available
        ? [
            {'role': 'assistant', 'fullText': '현재 ${target.paneId} 답변'},
          ]
        : [],
    'nextCursor': null,
  },
  'modelPicker': {'supported': provider != null, 'mode': 'native-cli'},
};

http.Response response(Object value, [int code = 200]) => http.Response(
  jsonEncode(value),
  code,
  headers: {'content-type': 'application/json; charset=utf-8'},
);

ProviderContainer setup(DesktopClient client) => ProviderContainer(
  overrides: [
    desktopClientProvider.overrideWithValue(client),
    connectionStoreProvider.overrideWithValue(
      FakeConnectionStore(
        initial: const SavedConnections(
          connections: [pcA, pcB],
          activeId: 'a:19746',
        ),
      ),
    ),
  ],
);

Future<void> pumpChat(
  WidgetTester tester,
  ProviderContainer container, {
  TextScaler textScaler = TextScaler.noScaling,
}) async {
  await tester.pumpWidget(
    UncontrolledProviderScope(
      container: container,
      child: MaterialApp(
        theme: buildAppTheme(),
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(context).copyWith(textScaler: textScaler),
          child: child!,
        ),
        home: Scaffold(
          body: SafeArea(
            child: PaneChatView(target: target, onReturnToTerminal: () {}),
          ),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  test('정확한 pane 조회와 prompt·모델·키·중단은 인증과 대상 토큰을 실제 계약대로 보낸다', () async {
    final requests = <http.Request>[];
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        requests.add(request);
        expect(request.headers['authorization'], 'Bearer token-a');
        if (request.method == 'GET') {
          return response({'success': true, 'snapshot': snapshotJson()});
        }
        return response({'success': true});
      }),
    );
    addTearDown(client.close);
    final snapshot = await client.fetchPaneChat(pcA, target);
    expect(snapshot.transcript.messages.single.text, '현재 %19 답변');
    expect(snapshot.inputState, PaneChatInputState.ready);
    expect(requests.single.url.pathSegments, [
      'api',
      'mobile',
      'tasks',
      'task/1',
      'pane-chat',
    ]);
    expect(requests.single.url.queryParameters, {'paneId': '%19'});
    await client.sendPaneChatAction(
      pcA,
      target.taskId,
      snapshot.targetToken!,
      PaneChatAction.prompt,
      text: '여러 줄\n질문',
    );
    await client.sendPaneChatAction(
      pcA,
      target.taskId,
      snapshot.targetToken!,
      PaneChatAction.modelPicker,
    );
    await client.sendPaneChatAction(
      pcA,
      target.taskId,
      snapshot.targetToken!,
      PaneChatAction.key,
      key: PaneChatKey.down,
    );
    await client.sendPaneChatAction(
      pcA,
      target.taskId,
      snapshot.targetToken!,
      PaneChatAction.interrupt,
    );
    expect(requests.skip(1).map((request) => request.url.pathSegments.last), [
      'prompt',
      'model-picker',
      'key',
      'interrupt',
    ]);
    expect(requests.skip(1).map((request) => jsonDecode(request.body)), [
      {'targetToken': 'target-a', 'text': '여러 줄\n질문'},
      {'targetToken': 'target-a'},
      {'targetToken': 'target-a', 'key': 'down'},
      {'targetToken': 'target-a'},
    ]);
  });

  test('서버가 다른 pane을 돌려주면 기록과 입력 대상을 사용할 수 없다', () async {
    final client = DesktopClient(
      httpClient: MockClient(
        (_) async => response({
          'snapshot': snapshotJson(
            target: (taskId: target.taskId, paneId: '%20'),
          ),
        }),
      ),
    );
    addTearDown(client.close);
    await expectLater(client.fetchPaneChat(pcA, target), throwsFormatException);
  });

  test('중복 조회와 전송 중 두 번째 전송을 막는다', () async {
    var reads = 0;
    var sends = 0;
    final delayedRead = Completer<http.Response>();
    final delayedSend = Completer<http.Response>();
    final readStarted = Completer<void>();
    final sendStarted = Completer<void>();
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        if (request.method == 'GET') {
          if (++reads == 2) {
            readStarted.complete();
            return delayedRead.future;
          }
          return response({'snapshot': snapshotJson()});
        }
        sends++;
        sendStarted.complete();
        return delayedSend.future;
      }),
    );
    final container = setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    final provider = paneChatProvider(target);
    final sub = container.listen(provider, (_, _) {});
    addTearDown(sub.close);
    await container.read(provider.future);
    final controller = container.read(provider.notifier);
    final refresh = controller.refresh();
    await readStarted.future;
    await controller.refresh();
    expect(reads, 2);
    expect(sends, 0);
    delayedRead.complete(response({'snapshot': snapshotJson()}));
    await refresh;
    final send = controller.send('첫 질문');
    await sendStarted.future;
    expect(await controller.send('중복 질문'), isFalse);
    expect(sends, 1);
    delayedSend.complete(response({'success': true}));
    expect(await send, isTrue);
    expect(reads, 3);
  });

  for (final changesTarget in [false, true]) {
    test(
      '수동 동작은 진행 중인 조회를 기다리고 ${changesTarget ? '대상이 바뀌면 전송하지 않는다' : '같은 대상이면 한 번만 전송한다'}',
      () async {
        var reads = 0;
        final delayed = Completer<http.Response>();
        final readStarted = Completer<void>();
        final actions = <Map<String, dynamic>>[];
        final client = DesktopClient(
          httpClient: MockClient((request) async {
            if (request.method == 'GET') {
              if (++reads == 2) {
                readStarted.complete();
                return delayed.future;
              }
              return response({'snapshot': snapshotJson()});
            }
            actions.add({
              'action': request.url.pathSegments.last,
              ...jsonDecode(request.body) as Map<String, dynamic>,
            });
            return response({'success': true});
          }),
        );
        final container = setup(client);
        addTearDown(container.dispose);
        addTearDown(client.close);
        final provider = paneChatProvider(target);
        final sub = container.listen(provider, (_, _) {});
        addTearDown(sub.close);
        await container.read(provider.future);
        final controller = container.read(provider.notifier);
        final refreshing = controller.refresh();
        await readStarted.future;
        final wasUsable = container.read(provider).requireValue.canPrompt;
        final action = controller.openModelPicker();
        await Future<void>.value();
        expect(actions, isEmpty);
        delayed.complete(
          response({
            'snapshot': snapshotJson(
              token: changesTarget ? 'target-b' : 'target-a',
            ),
          }),
        );
        await refreshing;
        expect(await action, !changesTarget);
        expect(wasUsable, isTrue);
        expect(
          actions,
          changesTarget
              ? isEmpty
              : [
                  {'action': 'model-picker', 'targetToken': 'target-a'},
                ],
        );
      },
    );
  }

  test('CLI 입력 중에는 prompt·모델을 막고 실제 방향키와 중단은 허용한다', () async {
    final actions = <String>[];
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        if (request.method == 'GET') {
          return response({'snapshot': snapshotJson(inputState: 'occupied')});
        }
        actions.add(request.url.pathSegments.last);
        return response({'success': true});
      }),
    );
    final container = setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    final provider = paneChatProvider(target);
    final sub = container.listen(provider, (_, _) {});
    addTearDown(sub.close);
    await container.read(provider.future);
    final controller = container.read(provider.notifier);
    expect(await controller.send('새 질문'), isFalse);
    expect(await controller.openModelPicker(), isFalse);
    expect(await controller.sendKey(PaneChatKey.enter), isTrue);
    expect(await controller.interrupt(), isTrue);
    expect(actions, ['key', 'interrupt']);
  });

  test('조회 실패 이후에는 마지막 화면을 유지하되 전송을 막고 재조회로 복구한다', () async {
    var reads = 0;
    var sends = 0;
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        if (request.method == 'POST') {
          sends++;
          return response({'success': true});
        }
        if (++reads == 2) return response({'error': 'offline'}, 503);
        return response({
          'snapshot': snapshotJson(
            token: reads > 2 ? 'new-target' : 'target-a',
          ),
        });
      }),
    );
    final container = setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    final provider = paneChatProvider(target);
    final sub = container.listen(provider, (_, _) {});
    addTearDown(sub.close);
    await container.read(provider.future);
    final controller = container.read(provider.notifier);
    await controller.refresh();
    expect(container.read(provider).requireValue.isStale, isTrue);
    expect(
      container.read(provider).requireValue.snapshot.screen,
      '› 현재 CLI 화면',
    );
    expect(await controller.send('오래된 대상 입력'), isFalse);
    expect(sends, 0);
    await controller.refresh();
    expect(
      container.read(provider).requireValue.snapshot.targetToken,
      'new-target',
    );
    expect(container.read(provider).requireValue.canPrompt, isTrue);
  });

  test('PC 변경 뒤 늦게 끝난 이전 PC 조회는 현재 CLI를 덮어쓰지 않는다', () async {
    final late = Completer<http.Response>();
    var aReads = 0;
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        if (request.url.host == 'a' && ++aReads == 2) return late.future;
        return response({
          'snapshot': snapshotJson(
            token: request.url.host,
            screen: request.url.host,
          ),
        });
      }),
    );
    final container = setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    final provider = paneChatProvider(target);
    final sub = container.listen(provider, (_, _) {});
    addTearDown(sub.close);
    await container.read(provider.future);
    final refresh = container.read(provider.notifier).refresh();
    await container.read(connectionControllerProvider.notifier).select(pcB.id);
    await container.read(provider.future);
    late.complete(
      response({'snapshot': snapshotJson(token: 'old', screen: 'old')}),
    );
    await refresh;
    expect(container.read(provider).requireValue.connectionId, pcB.id);
    expect(container.read(provider).requireValue.snapshot.screen, 'b');
  });

  test('401은 요청한 PC만 해제하고 다른 PC 연결은 보존한다', () async {
    final client = DesktopClient(
      httpClient: MockClient(
        (_) async => response({'error': 'unauthorized'}, 401),
      ),
    );
    final container = setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    final provider = paneChatProvider(target);
    final sub = container.listen(provider, (_, _) {});
    addTearDown(sub.close);
    await expectLater(container.read(provider.future), throwsA(anything));
    final registry = await container.read(connectionControllerProvider.future);
    expect(registry.find(pcA.id), isNull);
    expect(registry.find(pcB.id), isNotNull);
  });

  testWidgets('전송 실패는 초안을 유지하고 수락된 전송만 초안을 비운다', (tester) async {
    var sends = 0;
    final bodies = <String>[];
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        if (request.method == 'GET') {
          return response({'snapshot': snapshotJson()});
        }
        bodies.add((jsonDecode(request.body) as Map)['text'] as String);
        if (++sends == 1) return response({'error': 'CLI 전송 실패'}, 500);
        return response({'success': true});
      }),
    );
    final container = setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    await pumpChat(tester, container);
    await tester.enterText(find.byType(TextField), '보존할\n여러 줄 질문');
    await tester.pump();
    await tester.tap(find.byTooltip('메시지 전송'));
    await tester.pumpAndSettle();
    expect(find.text('CLI 전송 실패'), findsOneWidget);
    expect(
      tester.widget<TextField>(find.byType(TextField)).controller!.text,
      '보존할\n여러 줄 질문',
    );
    await tester.tap(find.byTooltip('메시지 전송'));
    await tester.pumpAndSettle();
    expect(
      tester.widget<TextField>(find.byType(TextField)).controller!.text,
      isEmpty,
    );
    expect(bodies, ['보존할\n여러 줄 질문', '보존할\n여러 줄 질문']);
    await tester.pumpWidget(const SizedBox.shrink());
    container.dispose();
    await tester.pump();
  });

  testWidgets('변경된 CLI 토큰은 이전 초안을 새 대상에 보내지 않는다', (tester) async {
    var token = 'target-a';
    var sends = 0;
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        if (request.method == 'GET') {
          return response({'snapshot': snapshotJson(token: token)});
        }
        sends++;
        return response({'error': 'CLI 대상이 변경되었습니다.'}, 409);
      }),
    );
    final container = setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    await pumpChat(tester, container);
    await tester.enterText(find.byType(TextField), '이전 CLI 초안');
    await tester.pump();
    await tester.tap(find.byTooltip('메시지 전송'));
    await tester.pumpAndSettle();
    expect(
      tester.widget<TextField>(find.byType(TextField)).controller!.text,
      '이전 CLI 초안',
    );
    expect(
      container.read(paneChatProvider(target)).requireValue.isStale,
      isTrue,
    );
    token = 'target-b';
    await container.read(paneChatProvider(target).notifier).refresh();
    await tester.pumpAndSettle();
    expect(
      tester.widget<TextField>(find.byType(TextField)).controller!.text,
      isEmpty,
    );
    expect(sends, 1);
    await tester.pumpWidget(const SizedBox.shrink());
    container.dispose();
    await tester.pump();
  });

  testWidgets('모델 선택은 실제 CLI 화면과 방향키·확인을 사용하고 기록으로 대체하지 않는다', (tester) async {
    var occupied = false;
    final actions = <Map<String, dynamic>>[];
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        if (request.method == 'GET') {
          return response({
            'snapshot': snapshotJson(
              available: false,
              inputState: occupied ? 'occupied' : 'ready',
              screen: occupied ? '모델 선택 화면: GPT-5' : '› 질문을 입력하세요',
            ),
          });
        }
        actions.add({
          'action': request.url.pathSegments.last,
          ...jsonDecode(request.body) as Map<String, dynamic>,
        });
        if (request.url.pathSegments.last == 'model-picker') occupied = true;
        return response({'success': true});
      }),
    );
    final container = setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    await pumpChat(tester, container);
    expect(find.textContaining('대화 기록을 아직 연결하지'), findsOneWidget);
    await tester.tap(find.text('모델'));
    await tester.pumpAndSettle();
    expect(find.text('모델 선택 화면: GPT-5'), findsOneWidget);
    await tester.tap(find.byTooltip('아래'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('확인'));
    await tester.pumpAndSettle();
    expect(actions, [
      {'action': 'model-picker', 'targetToken': 'target-a'},
      {'action': 'key', 'targetToken': 'target-a', 'key': 'down'},
      {'action': 'key', 'targetToken': 'target-a', 'key': 'enter'},
    ]);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
    container.dispose();
    await tester.pump();
  });

  testWidgets('짧은 실제 CLI 선택기는 작은 surface에 키를 모으고 긴 원문은 스크롤한다', (tester) async {
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    var screen =
        'Select model\n1. fixture-fast\n› 2. fixture-deep\n↑ ↓ choose\nEnter confirm · Esc cancel';
    final client = DesktopClient(
      httpClient: MockClient(
        (request) async => response({
          'snapshot': snapshotJson(inputState: 'occupied', screen: screen),
        }),
      ),
    );
    final container = setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    for (final size in const [
      Size(375, 812),
      Size(430, 932),
      Size(812, 375),
      Size(375, 260),
    ]) {
      tester.view.physicalSize = size;
      await pumpChat(tester, container);
      if (size.height >= 360) expect(find.text('현재 %19 답변'), findsOneWidget);
      final rawScroll = find
          .ancestor(
            of: find.text(screen),
            matching: find.byType(SingleChildScrollView),
          )
          .first;
      final rawRect = tester.getRect(rawScroll);
      expect(rawRect.height, lessThan(size.height * 0.25));
      final shell = find
          .ancestor(of: rawScroll, matching: find.byType(Container))
          .last;
      if (size.height > size.width) {
        expect(
          tester.getSize(shell).height,
          lessThanOrEqualTo(size.height * 0.45),
        );
      }
      for (final key in ['위', '아래', '왼쪽', '오른쪽', '확인', '취소']) {
        final rect = tester.getRect(find.byTooltip(key));
        expect(rect.width, greaterThanOrEqualTo(48));
        expect(rect.height, greaterThanOrEqualTo(48));
        expect(rect.bottom, lessThanOrEqualTo(size.height));
        expect(rect.top, greaterThanOrEqualTo(rawRect.bottom));
      }
      expect(tester.takeException(), isNull);
    }
    tester.view.physicalSize = const Size(375, 812);
    await pumpChat(tester, container, textScaler: const TextScaler.linear(1.8));
    for (final key in ['확인', '취소']) {
      expect(tester.getRect(find.byTooltip(key)).bottom, lessThan(812));
    }
    expect(tester.takeException(), isNull);
    screen = List.generate(80, (index) => 'actual CLI line $index').join('\n');
    await container.read(paneChatProvider(target).notifier).refresh();
    await tester.pumpAndSettle();
    final rawScroll = find
        .ancestor(
          of: find.text(screen),
          matching: find.byType(SingleChildScrollView),
        )
        .first;
    await tester.drag(rawScroll, const Offset(0, -5000));
    await tester.pumpAndSettle();
    final scrollable = find.descendant(
      of: rawScroll,
      matching: find.byType(Scrollable),
    );
    expect(
      tester.state<ScrollableState>(scrollable.first).position.pixels,
      greaterThan(0),
    );
    expect(find.text(screen), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
    container.dispose();
    await tester.pump();
  });

  testWidgets('모델·CLI·중단·전송은 입력과 한 shell에 있고 키보드 위에 남는다', (tester) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(375, 812);
    tester.view.viewPadding = const FakeViewPadding(bottom: 24);
    tester.view.viewInsets = const FakeViewPadding(bottom: 300);
    addTearDown(tester.view.reset);
    final client = DesktopClient(
      httpClient: MockClient(
        (request) async => response({'snapshot': snapshotJson()}),
      ),
    );
    final container = setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    await pumpChat(tester, container);
    await tester.enterText(find.byType(TextField), '키보드 초안');
    await tester.pumpAndSettle();
    final shell = find
        .ancestor(of: find.byType(TextField), matching: find.byType(Container))
        .first;
    final shellRect = tester.getRect(shell);
    for (final control in [
      find.text('모델'),
      find.text('CLI'),
      find.byTooltip('중단'),
      find.byTooltip('메시지 전송'),
    ]) {
      final rect = tester.getRect(control);
      expect(shellRect.contains(rect.center), isTrue);
      expect(rect.bottom, lessThanOrEqualTo(512));
    }
    final send = tester.widget<IconButton>(
      find.byWidgetPredicate(
        (widget) => widget is IconButton && widget.tooltip == '메시지 전송',
      ),
    );
    expect(send.onPressed, isNotNull);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
    container.dispose();
    await tester.pump();
  });

  testWidgets('키보드가 compact 경계를 왕복해도 입력 포커스와 초안을 유지한다', (tester) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(375, 600);
    addTearDown(tester.view.reset);
    final client = DesktopClient(
      httpClient: MockClient(
        (request) async => response({'snapshot': snapshotJson()}),
      ),
    );
    final container = setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    await pumpChat(tester, container);
    await tester.enterText(find.byType(TextField), 'preserved draft');
    await tester.pumpAndSettle();
    final editable = find.descendant(
      of: find.byType(TextField),
      matching: find.byType(EditableText),
    );
    final focusNode = tester.widget<EditableText>(editable).focusNode;
    expect(focusNode.hasFocus, isTrue);
    expect(tester.testTextInput.isVisible, isTrue);
    for (final inset in [300.0, 0.0]) {
      tester.view.viewInsets = FakeViewPadding(bottom: inset);
      await tester.pumpAndSettle();
      final input = tester.widget<EditableText>(editable);
      expect(input.focusNode, same(focusNode));
      expect(input.focusNode.hasFocus, isTrue);
      expect(tester.testTextInput.isVisible, isTrue);
      expect(input.controller.text, 'preserved draft');
      expect(
        tester.getRect(find.byTooltip('메시지 전송')).bottom,
        lessThanOrEqualTo(600 - inset),
      );
      expect(tester.takeException(), isNull);
    }
    await tester.enterText(find.byType(TextField), 'preserved draft continued');
    expect(
      tester.widget<EditableText>(editable).controller.text,
      'preserved draft continued',
    );
    await tester.pumpWidget(const SizedBox.shrink());
    container.dispose();
    await tester.pump();
  });

  testWidgets('최신 메시지에서 한 화면 넘게 올라가면 최신 이동 버튼이 나오고 누르면 최신으로 돌아간다', (
    tester,
  ) async {
    final client = DesktopClient(
      httpClient: MockClient((_) async {
        final snapshot = snapshotJson();
        (snapshot['transcript'] as Map<String, dynamic>)['messages'] = [
          {'role': 'assistant', 'fullText': '가장 최신 답변'},
          for (var index = 1; index < 40; index++)
            {'role': 'assistant', 'fullText': '이전 답변 $index'},
        ];
        return response({'snapshot': snapshot});
      }),
    );
    final container = setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    await pumpChat(tester, container);
    expect(find.text('가장 최신 답변'), findsOneWidget);
    expect(find.byTooltip('최신 메시지로 이동'), findsNothing);

    await tester.drag(find.text('가장 최신 답변'), const Offset(0, 100));
    await tester.pumpAndSettle();
    expect(
      find.byTooltip('최신 메시지로 이동'),
      findsNothing,
      reason: '한 화면 안쪽으로만 올라간 동안에는 버튼을 띄우지 않는다',
    );

    await tester.fling(find.byType(ListView), const Offset(0, 600), 4000);
    await tester.pumpAndSettle();
    expect(find.text('가장 최신 답변'), findsNothing);
    expect(find.byTooltip('최신 메시지로 이동'), findsOneWidget);

    await tester.tap(find.byTooltip('최신 메시지로 이동'));
    await tester.pumpAndSettle();
    expect(find.text('가장 최신 답변'), findsOneWidget);
    expect(find.byTooltip('최신 메시지로 이동'), findsNothing);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
    container.dispose();
    await tester.pump();
  });

  testWidgets('새 응답이 앞에 추가되어도 펼친 추론은 유지된다', (tester) async {
    var withNewReply = false;
    final client = DesktopClient(
      httpClient: MockClient((_) async {
        final snapshot = snapshotJson();
        (snapshot['transcript'] as Map<String, dynamic>)['messages'] = [
          if (withNewReply) {'role': 'assistant', 'fullText': '새 응답'},
          {
            'role': 'reasoning',
            'timestamp': '2026-11-12T10:00:00Z',
            'fullText': '추론 미리보기\n\n읽던 추론 전문',
          },
        ];
        return response({'snapshot': snapshot});
      }),
    );
    final container = setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    await pumpChat(tester, container);
    expect(find.text('추론 미리보기'), findsOneWidget);
    expect(find.text('읽던 추론 전문'), findsNothing);
    await tester.tap(find.text('추론 과정'));
    await tester.pumpAndSettle();
    expect(find.text('읽던 추론 전문'), findsOneWidget);
    withNewReply = true;
    await container.read(paneChatProvider(target).notifier).refresh();
    await tester.pumpAndSettle();
    expect(find.text('읽던 추론 전문'), findsOneWidget);
    expect(find.text('새 응답'), findsOneWidget);
    await tester.pumpWidget(const SizedBox.shrink());
    container.dispose();
    await tester.pump();
  });

  for (final (name, command, readable) in [
    ('work', '', 'work'),
    ('', 'codex', 'codex'),
    ('', '', 'pane'),
  ]) {
    testWidgets('채팅 탭은 중복·빈 이름을 구별하고 정확한 pane과 초안을 유지한다: $readable', (
      tester,
    ) async {
      const ids = ['same-uuid-prefix-0001', 'same-uuid-prefix-0002'];
      final client = _PaneFixtureClient(
        surfaces: TaskSurfaces(
          tabs: [
            MirrorTab(
              id: 'tab',
              name: name,
              panes: [
                for (final id in ids)
                  MirrorPane(
                    id: id,
                    tabId: 'tab',
                    tabName: name,
                    command: command,
                    left: 0,
                    top: 0,
                    width: 80,
                    height: 24,
                  ),
              ],
            ),
          ],
        ),
      );
      await tester.pumpWidget(
        wrapWithApp(
          const TaskDetailScreen(
            task: BoardTask(
              id: 'task-1',
              title: '현재 CLI',
              status: TaskStatus.progress,
            ),
          ),
          client: client,
          size: phoneSize,
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text('채팅', findRichText: true));
      await tester.pumpAndSettle();
      expect(find.text('1 · $readable'), findsOneWidget);
      expect(find.text('2 · $readable'), findsOneWidget);
      expect(find.text('1 · $readable · codex'), findsOneWidget);
      expect(find.byTooltip(ids.first), findsWidgets);
      await tester.enterText(find.byType(TextField).last, 'first pane draft');
      await tester.tap(find.text('2 · $readable'));
      await tester.pumpAndSettle();
      expect(find.byType(PaneChatView), findsNothing);
      expect(
        client.readTargets.every((target) => target.paneId == ids.first),
        isTrue,
      );
      await tester.tap(find.text('채팅', findRichText: true));
      await tester.pumpAndSettle();
      expect(client.readTargets.last.paneId, ids.last);
      expect(find.text('2 · $readable · codex'), findsOneWidget);
      expect(
        tester.widget<TextField>(find.byType(TextField).last).controller!.text,
        isEmpty,
      );
      await tester.tap(find.text('1 · $readable'));
      await tester.pumpAndSettle();
      expect(
        tester.widget<TextField>(find.byType(TextField).last).controller!.text,
        'first pane draft',
      );
      expect(
        tester.getSize(find.byTooltip('탭 닫기').first).height,
        greaterThanOrEqualTo(48),
      );
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
    });
  }

  for (final platform in [TargetPlatform.android, TargetPlatform.macOS]) {
    testWidgets('외부 키보드 채팅 전환은 선택 pane만 바꾸고 히스토리에서는 멈춘다: $platform', (
      tester,
    ) async {
      tester.view.physicalSize = phoneSize;
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      final client = _PaneFixtureClient();
      await tester.pumpWidget(
        wrapWithApp(
          const TaskDetailScreen(
            task: BoardTask(
              id: 'task-1',
              title: '현재 CLI',
              status: TaskStatus.progress,
            ),
          ),
          client: client,
          size: phoneSize,
        ),
      );
      await tester.pumpAndSettle();
      final modifier = platform == TargetPlatform.macOS
          ? LogicalKeyboardKey.metaLeft
          : LogicalKeyboardKey.controlLeft;
      Future<void> toggle() async {
        await tester.sendKeyDownEvent(modifier);
        await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
        await tester.sendKeyEvent(LogicalKeyboardKey.keyJ);
        await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
        await tester.sendKeyUpEvent(modifier);
        await tester.pumpAndSettle();
      }

      await tester.tap(find.bySemanticsIdentifier('pane-terminal'));
      await tester.pumpAndSettle();
      final inputBeforeToggle = [...?client.writtenInput['%19']];
      await toggle();
      expect(client.writtenInput['%19'] ?? [], inputBeforeToggle);
      expect(find.text('현재 %19 답변'), findsOneWidget);
      await tester.enterText(find.byType(TextField).last, 'A draft');
      await tester.tap(find.bySemanticsIdentifier('terminal-tab-%20'));
      await tester.pumpAndSettle();
      expect(find.byType(PaneChatView), findsNothing);
      await toggle();
      expect(find.text('현재 %20 답변'), findsOneWidget);
      await toggle();
      expect(find.byType(PaneChatView), findsNothing);
      await tester.tap(find.bySemanticsIdentifier('terminal-tab-%19'));
      await tester.pumpAndSettle();
      expect(
        tester.widget<TextField>(find.byType(TextField).last).controller!.text,
        'A draft',
      );
      await tester.tap(find.byTooltip('태스크 메뉴'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('대화 기록'));
      await tester.pumpAndSettle();
      await toggle();
      await tester.pageBack();
      await tester.pumpAndSettle();
      expect(find.text('현재 %19 답변'), findsOneWidget);
      expect(
        tester.widget<TextField>(find.byType(TextField).last).controller!.text,
        'A draft',
      );
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
    }, variant: TargetPlatformVariant({platform}));
  }

  testWidgets('선택 pane을 바꾸면 입력을 분리하고 터미널 전환은 원래 스트림을 유지한다', (tester) async {
    tester.view.physicalSize = const Size(390, 844);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final client = _PaneFixtureClient();
    await tester.pumpWidget(
      wrapWithApp(
        const TaskDetailScreen(
          task: BoardTask(
            id: 'task-1',
            title: '현재 CLI',
            status: TaskStatus.progress,
          ),
        ),
        client: client,
        size: phoneSize,
      ),
    );
    await tester.pumpAndSettle();
    expect(client.openedPanes.keys, contains('%19'));
    await tester.tap(find.text('채팅', findRichText: true));
    await tester.pumpAndSettle();
    expect(find.text('현재 %19 답변'), findsOneWidget);
    expect(client.closedPanes, isEmpty);
    final resizeCount = client.resizedPanes['%19']!.length;
    tester.view.viewInsets = const FakeViewPadding(bottom: 250);
    addTearDown(tester.view.resetViewInsets);
    await tester.pumpAndSettle();
    expect(client.resizedPanes['%19']!.length, resizeCount);
    await tester.enterText(find.byType(TextField).last, '첫 pane 초안');
    await tester.tap(find.bySemanticsIdentifier('terminal-tab-%20'));
    await tester.pumpAndSettle();
    expect(find.text('현재 %20 답변'), findsNothing);
    expect(find.byType(PaneChatView), findsNothing);
    expect(client.readTargets.map((value) => value.paneId), ['%19']);
    await tester.tap(find.text('채팅', findRichText: true));
    await tester.pumpAndSettle();
    expect(find.text('현재 %20 답변'), findsOneWidget);
    expect(
      tester.widget<TextField>(find.byType(TextField).last).controller!.text,
      isEmpty,
    );
    expect(client.readTargets.map((value) => value.paneId), ['%19', '%20']);
    await tester.tap(
      find.byWidgetPredicate(
        (widget) =>
            widget is Semantics &&
            widget.properties.identifier == 'terminal-tab-%19',
      ),
    );
    await tester.pumpAndSettle();
    expect(
      tester.widget<TextField>(find.byType(TextField).last).controller!.text,
      '첫 pane 초안',
    );
    final beforeReturn = client.openedPanes['%19'];
    await tester.tap(find.text('터미널', findRichText: true));
    await tester.pumpAndSettle();
    expect(identical(beforeReturn, client.openedPanes['%19']), isTrue);
    final readsWhileHidden = client.readTargets.length;
    await tester.pump(const Duration(seconds: 3));
    expect(client.readTargets.length, readsWhileHidden);
    await tester.tap(find.text('채팅', findRichText: true));
    await tester.pumpAndSettle();
    expect(
      tester.widget<TextField>(find.byType(TextField).last).controller!.text,
      '첫 pane 초안',
    );
    await tester.tap(find.byTooltip('태스크 메뉴'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('대화 기록'));
    await tester.pumpAndSettle();
    final readsInHistory = client.readTargets.length;
    await tester.pump(const Duration(seconds: 3));
    expect(client.readTargets.length, readsInHistory);
    await tester.pageBack();
    await tester.pumpAndSettle();
    expect(
      tester.widget<TextField>(find.byType(TextField).last).controller!.text,
      '첫 pane 초안',
    );
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
  });
}

class _PaneFixtureClient extends FakeDesktopClient {
  _PaneFixtureClient({TaskSurfaces? surfaces})
    : super(
        surfaces:
            surfaces ??
            const TaskSurfaces(
              canResize: true,
              tabs: [
                MirrorTab(
                  id: '@1',
                  name: 'main',
                  panes: [
                    MirrorPane(
                      id: '%19',
                      tabId: '@1',
                      tabName: 'main',
                      command: 'codex',
                      left: 0,
                      top: 0,
                      width: 80,
                      height: 24,
                    ),
                    MirrorPane(
                      id: '%20',
                      tabId: '@1',
                      tabName: 'main',
                      command: 'zsh',
                      left: 80,
                      top: 0,
                      width: 80,
                      height: 24,
                    ),
                  ],
                ),
              ],
            ),
      );
  final readTargets = <PaneChatTarget>[];

  @override
  Future<PaneChatSnapshot> fetchPaneChat(
    DesktopConnection connection,
    PaneChatTarget target,
  ) async {
    readTargets.add(target);
    return PaneChatSnapshot.fromJson(snapshotJson(target: target));
  }
}
