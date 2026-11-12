import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/chat_history/application/chat_history_controller.dart';
import 'package:kanvibe_mobile/src/features/chat_history/domain/chat_history.dart';
import 'package:kanvibe_mobile/src/features/chat_history/presentation/chat_history_screen.dart';
import 'package:kanvibe_mobile/src/features/chat_history/presentation/chat_message_tile.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';

import '../../../support/fake_desktop.dart';

const pcA = DesktopConnection(host: 'a', port: 9736, token: 'a');
const pcB = DesktopConnection(host: 'b', port: 9736, token: 'b');
const session = ChatSession(
  id: 'session/1',
  provider: 'hermes',
  title: '고친 대화',
);

Map<String, dynamic> history({
  String id = 'session/1',
  String title = '고친 대화',
  String? cursor,
}) => {
  'history': {
    'sessions': [
      {'id': id, 'provider': 'hermes', 'title': title},
    ],
    'sources': [
      {'provider': 'hermes', 'available': true},
    ],
    'nextCursor': cursor,
  },
};
http.Response jsonResponse(Object value) => http.Response(
  jsonEncode(value),
  200,
  headers: {'content-type': 'application/json; charset=utf-8'},
);

ProviderContainer setup(DesktopClient client) => ProviderContainer(
  overrides: [
    desktopClientProvider.overrideWithValue(client),
    connectionStoreProvider.overrideWithValue(
      FakeConnectionStore(
        initial: const SavedConnections(
          connections: [pcA, pcB],
          activeId: 'a:9736',
        ),
      ),
    ),
  ],
);

void main() {
  test('목록 요청은 인증·태스크 인코딩·검색·페이지를 전달하고 서버 결과를 파싱한다', () async {
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        expect(request.headers['authorization'], 'Bearer a');
        expect(
          request.url.toString(),
          'http://a:9736/api/mobile/tasks/task%2F1/ai-sessions?query=fix&cursor=20&limit=20',
        );
        return jsonResponse(history(cursor: '40'));
      }),
    );
    addTearDown(client.close);
    final page = await client.fetchChatHistory(
      pcA,
      'task/1',
      query: 'fix',
      cursor: '20',
    );
    expect(page.sessions.single.title, '고친 대화');
    expect(page.nextCursor, '40');
    expect(page.providers, ['hermes']);
  });

  test('상세 요청에는 파일 경로 없이 Agent와 session id를 인코딩하고 전문을 읽는다', () async {
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        expect(
          request.url.toString(),
          'http://a:9736/api/mobile/tasks/task-1/ai-sessions/hermes/session%2F1?cursor=20&limit=20',
        );
        return jsonResponse({
          'detail': {
            'messages': [
              {'role': 'assistant', 'text': '짧음', 'fullText': '전체 본문'},
            ],
            'nextCursor': null,
          },
        });
      }),
    );
    addTearDown(client.close);
    final page = await client.fetchChatMessages(
      pcA,
      'task-1',
      session,
      cursor: '20',
    );
    expect(page.messages.single.text, '전체 본문');
  });

  test('페이지 추가 실패는 기존 목록을 유지하고 재시도하면 중복 없이 합친다', () async {
    var attempts = 0;
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        if (!request.url.queryParameters.containsKey('cursor')) {
          return jsonResponse(history(cursor: '20'));
        }
        if (attempts++ == 0) return http.Response('', 503);
        return jsonResponse({
          'history': {
            'sessions': [
              {'id': 'session/1', 'provider': 'hermes', 'title': '중복'},
              {'id': 'second', 'provider': 'hermes', 'title': '두 번째'},
            ],
            'sources': [],
            'nextCursor': null,
          },
        });
      }),
    );
    final container = setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    final provider = chatHistoryProvider('task');
    final sub = container.listen(provider, (_, _) {});
    addTearDown(sub.close);
    await container.read(provider.future);
    await container.read(provider.notifier).loadMore();
    expect(container.read(provider).requireValue.page.sessions.length, 1);
    expect(container.read(provider).requireValue.loadMoreFailed, isTrue);
    await container.read(provider.notifier).loadMore();
    expect(
      container.read(provider).requireValue.page.sessions.map((s) => s.id),
      ['session/1', 'second'],
    );
  });

  test('PC 전환 전 시작한 이전 페이지가 새 PC 목록을 덮어쓰지 않는다', () async {
    final oldPage = Completer<http.Response>();
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        if (request.url.queryParameters.containsKey('cursor')) {
          return oldPage.future;
        }
        return jsonResponse(history(title: request.url.host, cursor: '20'));
      }),
    );
    final container = setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    final provider = chatHistoryProvider('task');
    final sub = container.listen(provider, (_, _) {});
    addTearDown(sub.close);
    await container.read(provider.future);
    final loading = container.read(provider.notifier).loadMore();
    await container.read(connectionControllerProvider.notifier).select(pcB.id);
    await container.read(provider.future);
    oldPage.complete(jsonResponse(history(id: 'old', title: 'old')));
    await loading;
    expect(
      container.read(provider).requireValue.page.sessions.single.title,
      'b',
    );
  });

  test('401은 요청한 PC만 해제한다', () async {
    final client = DesktopClient(
      httpClient: MockClient((_) async => http.Response('', 401)),
    );
    final container = setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    final sub = container.listen(chatHistoryProvider('task'), (_, _) {});
    addTearDown(sub.close);
    await expectLater(
      container.read(chatHistoryProvider('task').future),
      throwsA(isA<StateError>()),
    );
    final connections = await container.read(
      connectionControllerProvider.future,
    );
    expect(connections.find(pcA.id), isNull);
    expect(connections.find(pcB.id), isNotNull);
  });

  testWidgets('검색한 대화를 열어 전문을 읽고 이전 메시지를 추가한다', (tester) async {
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        if (request.url.path.endsWith('ai-sessions')) {
          return jsonResponse(history());
        }
        final older = request.url.queryParameters.containsKey('cursor');
        return jsonResponse({
          'detail': {
            'messages': [
              {
                'role': older ? 'user' : 'assistant',
                'fullText': older ? '이전 질문' : '전체 답변 본문',
              },
            ],
            'nextCursor': older ? null : '20',
          },
        });
      }),
    );
    final container = setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    await tester.pumpWidget(
      UncontrolledProviderScope(
        container: container,
        child: const MaterialApp(home: ChatHistoryScreen(taskId: 'task')),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.text('고친 대화'));
    await tester.pumpAndSettle();
    expect(find.text('전체 답변 본문'), findsOneWidget);
    await tester.tap(find.text('이전 메시지 보기'));
    await tester.pumpAndSettle();
    expect(find.text('이전 질문'), findsOneWidget);
    expect(find.byTooltip('메시지 복사'), findsNWidgets(2));
  });
  testWidgets('도구와 추론은 접어서 보여주고 펼치면 전문과 시간을 읽는다', (tester) async {
    final client = DesktopClient(
      httpClient: MockClient(
        (_) async => jsonResponse({
          'detail': {
            'messages': [
              {
                'role': 'reasoning',
                'fullText': '숨겨진 추론 전문',
                'timestamp': '2026-11-12T09:30:00Z',
              },
              {'role': 'tool', 'fullText': '숨겨진 도구 결과'},
              {'role': 'assistant', 'fullText': '읽을 답변'},
            ],
            'nextCursor': null,
          },
        }),
      ),
    );
    final container = setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    await tester.pumpWidget(
      UncontrolledProviderScope(
        container: container,
        child: const MaterialApp(
          home: ChatMessagesScreen(taskId: 'task', session: session),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('읽을 답변'), findsOneWidget);
    expect(find.text('숨겨진 추론 전문'), findsNothing);
    expect(find.text('숨겨진 도구 결과'), findsNothing);
    await tester.tap(find.text('추론 과정'));
    await tester.pumpAndSettle();
    expect(find.text('숨겨진 추론 전문'), findsOneWidget);
    final localTime = DateTime.parse('2026-11-12T09:30:00Z').toLocal();
    final timeLabel =
        '${localTime.hour.toString().padLeft(2, '0')}:${localTime.minute.toString().padLeft(2, '0')}';
    expect(
      find.byTooltip(
        '${localTime.year}. ${localTime.month}. ${localTime.day}. $timeLabel',
      ),
      findsOneWidget,
    );
  });

  testWidgets('넓은 화면은 목록 옆에 대화를 열고 검색 초기화가 서버 검색도 해제한다', (tester) async {
    tester.view.physicalSize = const Size(1200, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final queries = <String?>[];
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        if (request.url.path.endsWith('ai-sessions')) {
          queries.add(request.url.queryParameters['query']);
          return jsonResponse(history());
        }
        return jsonResponse({
          'detail': {
            'messages': [
              {'role': 'assistant', 'fullText': '옆에 열린 답변'},
            ],
            'nextCursor': null,
          },
        });
      }),
    );
    final container = setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    await tester.pumpWidget(
      UncontrolledProviderScope(
        container: container,
        child: const MaterialApp(home: ChatHistoryScreen(taskId: 'task')),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.text('고친 대화'));
    await tester.pumpAndSettle();
    expect(find.byType(ChatHistoryScreen), findsOneWidget);
    expect(find.text('옆에 열린 답변'), findsOneWidget);
    expect(find.byKey(const ValueKey('chat-session-list')), findsOneWidget);
    await tester.enterText(find.byType(TextField), 'fix');
    await tester.testTextInput.receiveAction(TextInputAction.search);
    await tester.pumpAndSettle();
    expect(queries.last, 'fix');
    await tester.tap(find.byTooltip('검색 지우기'));
    await tester.pumpAndSettle();
    expect(queries.last, isNull);
    expect(
      tester.widget<TextField>(find.byType(TextField)).controller!.text,
      isEmpty,
    );
    expect(tester.takeException(), isNull);
  });
  testWidgets('좁은 화면과 큰 글씨에서도 코드 전문을 읽고 목록으로 돌아간다', (tester) async {
    tester.view.physicalSize = const Size(390, 844);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        if (request.url.path.endsWith('ai-sessions')) {
          return jsonResponse(history());
        }
        return jsonResponse({
          'detail': {
            'messages': [
              {
                'role': 'assistant',
                'fullText':
                    '답변 본문\n```dart\nfinal value = "긴 코드 전문을 그대로 읽습니다";\n```',
              },
            ],
            'nextCursor': null,
          },
        });
      }),
    );
    final container = setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    await tester.pumpWidget(
      UncontrolledProviderScope(
        container: container,
        child: MaterialApp(
          builder: (context, child) => MediaQuery(
            data: MediaQuery.of(context)
                .copyWith(textScaler: const TextScaler.linear(2)),
            child: child!,
          ),
          home: const ChatHistoryScreen(taskId: 'task'),
        ),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.text('고친 대화'));
    await tester.pumpAndSettle();
    expect(find.text('답변 본문'), findsOneWidget);
    expect(find.text('final value = "긴 코드 전문을 그대로 읽습니다";'), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.pageBack();
    await tester.pumpAndSettle();
    expect(find.text('고친 대화'), findsOneWidget);
    expect(find.byType(TextField), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
  testWidgets('Markdown 제목·강조·코드를 렌더링하고 로컬 파일 링크와 이미지를 열지 않는다', (tester) async {
    tester.view.physicalSize = const Size(390, 844);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: MediaQuery(
            data: const MediaQueryData(textScaler: TextScaler.linear(2)),
            child: const SingleChildScrollView(
              child: Padding(
                padding: EdgeInsets.all(16),
                child: ChatMessageTile(
                  provider: 'codex',
                  message: ChatMessage(
                    role: 'assistant',
                    text: '## 변경 사항\n\n**중요한 결과**\n\n```dart\nfinal result = "아주 긴 한 줄의 코드도 본문 폭을 넘지 않고 스크롤합니다";\n```\n\n[로컬 파일](file:///private/secret.txt)\n\n![로컬 이미지](file:///private/secret.png)',
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('변경 사항'), findsOneWidget);
    expect(find.text('## 변경 사항'), findsNothing);
    expect(find.text('중요한 결과'), findsOneWidget);
    expect(find.text('**중요한 결과**'), findsNothing);
    expect(find.textContaining('final result'), findsOneWidget);
    expect(find.byType(Image), findsNothing);
    expect(find.textContaining('로컬 이미지'), findsOneWidget);
    await tester.ensureVisible(find.text('로컬 파일'));
    await tester.tap(find.text('로컬 파일'));
    await tester.pumpAndSettle();
    expect(find.text('HTTP 또는 HTTPS 링크만 열 수 있습니다.'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
}
