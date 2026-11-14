import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/features/chat_history/domain/chat_history.dart';
import 'package:kanvibe_mobile/src/features/chat_history/presentation/chat_history_screen.dart';

import 'chat_history_test.dart' as fixture;

void main() {
  testWidgets('미제출 초안은 역할 변경에도 유지되고 지우기는 실제 입력을 비운다', (tester) async {
    final requests = <http.Request>[];
    await mountMessages(tester, requests);
    await tester.enterText(find.byType(TextFormField), 'zqx');
    await tester.tap(find.widgetWithText(FilterChip, 'user'));
    await tester.pumpAndSettle();
    expect(input(tester), 'zqx');
    expect(
      requests.every((r) => !r.url.queryParameters.containsKey('query')),
      isTrue,
    );

    await tester.tap(find.byTooltip('검색 지우기'));
    await tester.pumpAndSettle();
    expect(input(tester), isEmpty);
    expect(
      requests.every((r) => !r.url.queryParameters.containsKey('query')),
      isTrue,
    );
    expect(
      tester
          .widget<FilterChip>(find.widgetWithText(FilterChip, 'user'))
          .selected,
      isTrue,
    );
  });

  testWidgets('제출 검색의 지우기는 역할을 보존하며 cursor 없이 전체 본문을 요청한다', (tester) async {
    final requests = <http.Request>[];
    await mountMessages(tester, requests);
    await tester.tap(find.widgetWithText(FilterChip, 'assistant'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextFormField), '  repair  ');
    await tester.testTextInput.receiveAction(TextInputAction.search);
    await tester.pumpAndSettle();
    expect(requests.last.url.queryParameters['query'], 'repair');
    expect(requests.last.url.queryParameters['roles'], 'assistant');
    expect(requests.last.url.queryParameters.containsKey('cursor'), isFalse);

    await tester.tap(find.text('이전 메시지 보기'));
    await tester.pumpAndSettle();
    expect(requests.last.url.queryParameters['cursor'], '20');
    await tester.enterText(find.byType(TextFormField), 'unsent draft');
    await tester.tap(find.byTooltip('검색 지우기'));
    await tester.pumpAndSettle();
    expect(input(tester), isEmpty);
    expect(requests.last.url.queryParameters['roles'], 'assistant');
    expect(requests.last.url.queryParameters.containsKey('query'), isFalse);
    expect(requests.last.url.queryParameters.containsKey('cursor'), isFalse);
    expect(
      requests.any((r) => r.url.queryParameters['query'] == 'unsent draft'),
      isFalse,
    );
    expect(
      tester
          .widget<FilterChip>(find.widgetWithText(FilterChip, 'assistant'))
          .selected,
      isTrue,
    );
  });

  testWidgets('동일한 빈 검색어의 다른 대화로 바뀌면 이전 초안을 재사용하지 않는다', (tester) async {
    final requests = <http.Request>[];
    final container = await mountMessages(tester, requests);
    await tester.enterText(find.byType(TextFormField), 'private draft');
    await tester.pumpWidget(
      messagesApp(
        container,
        const ChatSession(id: 'another', provider: 'hermes', title: '다른 대화'),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('다른 대화'), findsOneWidget);
    expect(input(tester), isEmpty);
    expect(requests.last.url.path.endsWith('/hermes/another'), isTrue);
    expect(
      requests.every((r) => !r.url.queryParameters.containsKey('query')),
      isTrue,
    );
  });
}

String input(WidgetTester tester) => tester
    .widget<EditableText>(
      find.descendant(
        of: find.byType(TextFormField),
        matching: find.byType(EditableText),
      ),
    )
    .controller
    .text;

Widget messagesApp(ProviderContainer container, ChatSession session) =>
    UncontrolledProviderScope(
      container: container,
      child: MaterialApp(
        home: ChatMessagesScreen(taskId: 'task', session: session),
      ),
    );

Future<ProviderContainer> mountMessages(
  WidgetTester tester,
  List<http.Request> requests,
) async {
  final client = DesktopClient(
    httpClient: MockClient((request) async {
      requests.add(request);
      return fixture.jsonResponse({
        'detail': {
          'messages': [
            {
              'role': request.url.queryParameters['roles'] ?? 'user',
              'fullText': '본문 검증 메시지',
            },
          ],
          'nextCursor': request.url.queryParameters.containsKey('cursor')
              ? null
              : '20',
        },
      });
    }),
  );
  final container = fixture.setup(client);
  await tester.pumpWidget(messagesApp(container, fixture.session));
  await tester.pumpAndSettle();
  addTearDown(() async {
    await tester.pumpWidget(const SizedBox.shrink());
    container.dispose();
    client.close();
  });
  return container;
}
