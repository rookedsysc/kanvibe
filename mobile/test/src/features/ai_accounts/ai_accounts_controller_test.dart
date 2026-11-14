import 'dart:async';
import 'dart:convert';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/connection/data/connection_store.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/ai_accounts/application/ai_accounts_controller.dart';
import 'package:kanvibe_mobile/src/features/ai_accounts/domain/ai_account.dart';

const pcA = DesktopConnection(host: 'a', port: 9746, token: 'a');
const pcB = DesktopConnection(host: 'b', port: 9746, token: 'b');

class MemoryConnections extends ConnectionStore {
  SavedConnections current = const SavedConnections(
    connections: [pcA, pcB],
    activeId: 'a:9746',
  );
  @override
  Future<SavedConnections> readRegistry() async => current;
  @override
  Future<void> writeRegistry(SavedConnections registry) async {
    current = registry;
  }
}

Map<String, dynamic> accounts(String label) => {
  'success': true,
  'accounts': [
    {
      'accountId': 'account-id',
      'provider': 'claude',
      'label': label,
      'isLoggedIn': true,
      'isRemovable': true,
    },
  ],
};
http.Response response(Object value, {int status = 200}) =>
    http.Response(jsonEncode(value), status);
ProviderContainer setup(Future<http.Response> Function(http.Request) handler) {
  final client = DesktopClient(httpClient: MockClient(handler));
  final container = ProviderContainer(
    overrides: [
      desktopClientProvider.overrideWithValue(client),
      connectionStoreProvider.overrideWithValue(MemoryConnections()),
    ],
  );
  container.listen(aiAccountsProvider, (_, _) {});
  container.listen(aiAccountActionsProvider, (_, _) {});
  addTearDown(() {
    container.dispose();
    client.close();
  });
  return container;
}

void main() {
  test('중복 추가를 막고 성공하면 목록을 재조회한다', () async {
    final added = Completer<http.Response>();
    var writes = 0;
    var registered = false;
    final container = setup((request) async {
      if (request.method == 'POST') {
        writes++;
        final result = await added.future;
        registered = true;
        return result;
      }
      return response(accounts(registered ? 'Created' : 'Original'));
    });
    await container.read(aiAccountsProvider.future);
    await container.read(aiAccountActionsProvider.future);
    final action = container.read(aiAccountActionsProvider.notifier);
    final first = action.add(AiAccountProvider.claude, 'work');
    expect(await action.add(AiAccountProvider.claude, 'work'), isNull);
    added.complete(response({'success': true, 'accountId': 'new-id'}));
    expect(await first, 'new-id');
    expect(writes, 1);
    expect(
      (await container.read(aiAccountsProvider.future)).single.label,
      'Created',
    );
  });
  test('추가 실패는 기존 목록을 유지하고 다시 시도할 수 있다', () async {
    var fail = true;
    final container = setup(
      (request) async => request.method == 'POST' && fail
          ? response({'success': false, 'error': '중복 계정'}, status: 409)
          : request.method == 'POST'
          ? response({'success': true, 'accountId': 'new-id'})
          : response(accounts('Original')),
    );
    await container.read(aiAccountsProvider.future);
    await container.read(aiAccountActionsProvider.future);
    final action = container.read(aiAccountActionsProvider.notifier);
    expect(await action.add(AiAccountProvider.codex, 'work'), isNull);
    expect(container.read(aiAccountActionsProvider).hasError, isTrue);
    expect(
      container.read(aiAccountsProvider).requireValue.single.label,
      'Original',
    );
    fail = false;
    expect(await action.add(AiAccountProvider.codex, 'other'), 'new-id');
  });
  test('PC 전환 뒤 도착한 이전 mutation은 새 PC의 목록을 덮지 않는다', () async {
    final pending = Completer<http.Response>();
    final container = setup(
      (request) async => request.method == 'POST'
          ? pending.future
          : response(accounts(request.url.host)),
    );
    await container.read(aiAccountsProvider.future);
    await container.read(aiAccountActionsProvider.future);
    final add = container
        .read(aiAccountActionsProvider.notifier)
        .add(AiAccountProvider.claude, 'work');
    await container.read(connectionControllerProvider.notifier).select(pcB.id);
    expect((await container.read(aiAccountsProvider.future)).single.label, 'b');
    pending.complete(response({'success': true, 'accountId': 'old-id'}));
    expect(await add, isNull);
    expect(container.read(aiAccountsProvider).requireValue.single.label, 'b');
  });
  test('401 응답은 현재 거절된 PC만 연결 목록에서 제거한다', () async {
    final container = setup(
      (request) async =>
          response({'success': false, 'error': 'unpaired'}, status: 401),
    );
    await expectLater(
      container.read(aiAccountsProvider.future),
      throwsA(isA<StateError>()),
    );
    final registry = await container.read(connectionControllerProvider.future);
    expect(registry.find(pcA.id), isNull);
    expect(registry.find(pcB.id), isNotNull);
  });
}
