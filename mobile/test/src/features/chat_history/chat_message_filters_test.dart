import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/features/chat_history/application/chat_history_controller.dart';

import 'chat_history_test.dart' as fixture;

void main() {
  test('역할 선택은 기존 페이지를 버리고 인증된 상세 요청에 전달한다', () async {
    final requests = <http.Request>[];
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        requests.add(request);
        return fixture.jsonResponse({
          'detail': {
            'messages': [
              {
                'role': request.url.queryParameters['roles'] ?? 'user',
                'fullText': '검증 본문',
              },
            ],
            'nextCursor': '20',
          },
        });
      }),
    );
    final container = fixture.setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    final target = (taskId: 'task', session: fixture.session);
    final provider = chatMessagesProvider(target);
    final subscription = container.listen(provider, (_, _) {});
    addTearDown(subscription.close);
    await container.read(provider.future);
    await container.read(provider.notifier).loadMore();
    expect(requests.last.url.queryParameters['cursor'], '20');
    container.read(chatMessageRolesProvider(target).notifier).toggle('tool');
    final result = await container.read(provider.future);
    expect(result.page.messages.single.role, 'tool');
    expect(requests.last.url.queryParameters['roles'], 'tool');
    expect(requests.last.url.queryParameters.containsKey('cursor'), isFalse);
    expect(requests.last.headers['authorization'], 'Bearer a');
    expect(
      jsonDecode(requests.last.body.isEmpty ? '{}' : requests.last.body),
      isEmpty,
    );
  });
  test('본문 검색은 역할 필터를 유지하며 새 cursor 없이 전송한다', () async {
    final requests = <http.Request>[];
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        requests.add(request);
        return fixture.jsonResponse({
          'detail': {'messages': [], 'nextCursor': null},
        });
      }),
    );
    final container = fixture.setup(client);
    addTearDown(container.dispose);
    addTearDown(client.close);
    final target = (taskId: 'task', session: fixture.session);
    final provider = chatMessagesProvider(target);
    final subscription = container.listen(provider, (_, _) {});
    addTearDown(subscription.close);
    await container.read(provider.future);
    container
        .read(chatMessageRolesProvider(target).notifier)
        .toggle('assistant');
    container
        .read(chatMessageQueryProvider(target).notifier)
        .search('  오류 수정  ');
    await container.read(provider.future);
    expect(requests.last.url.queryParameters['query'], '오류 수정');
    expect(requests.last.url.queryParameters['roles'], 'assistant');
    expect(requests.last.url.queryParameters.containsKey('cursor'), isFalse);
    container.read(chatMessageQueryProvider(target).notifier).search('');
    await container.read(provider.future);
    expect(requests.last.url.queryParameters.containsKey('query'), isFalse);
  });
}
