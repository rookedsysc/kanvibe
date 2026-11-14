import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/features/ai_accounts/data/ai_accounts_repository.dart';
import 'package:kanvibe_mobile/src/features/ai_accounts/domain/ai_account.dart';

const pc = DesktopConnection(host: 'fixture', port: 9746, token: 'paired');
const accountJson = {
  'accountId': 'opaque-id',
  'provider': 'claude',
  'accountName': 'work',
  'label': 'Work',
  'isLoggedIn': true,
  'planName': 'Pro',
  'isRemovable': true,
};
void main() {
  test('계정 조회는 공통 인증 전송을 사용하고 경로 없는 관리 ID를 읽는다', () async {
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        expect(request.url.path, '/api/mobile/ai/accounts');
        expect(request.headers['authorization'], 'Bearer paired');
        return http.Response(
          jsonEncode({
            'success': true,
            'accounts': [accountJson],
          }),
          200,
        );
      }),
    );
    addTearDown(client.close);
    final accounts = await AiAccountsRepository(client).fetchAccounts(pc);
    expect(accounts.single.accountId, 'opaque-id');
    expect(accounts.single.provider, AiAccountProvider.claude);
    expect(accounts.single.isRemovable, isTrue);
  });
  test('추가와 삭제는 서버가 발급한 ID와 허용된 제공자만 전송한다', () async {
    final requests = <http.Request>[];
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        requests.add(request);
        return http.Response(
          jsonEncode({'success': true, 'accountId': 'new-id', 'outcome': 'ok'}),
          200,
        );
      }),
    );
    addTearDown(client.close);
    final repository = AiAccountsRepository(client);
    expect(
      await repository.addAccount(pc, AiAccountProvider.codex, 'team'),
      'new-id',
    );
    await repository.removeAccount(pc, 'new-id');
    expect(jsonDecode(requests.first.body), {
      'provider': 'codex',
      'accountName': 'team',
    });
    expect(requests.last.method, 'DELETE');
    expect(requests.last.url.path, '/api/mobile/ai/accounts/new-id');
  });
  test('사용량은 기간·모델·구독·실패 이유와 갱신 시각을 보존한다', () async {
    final client = DesktopClient(
      httpClient: MockClient(
        (_) async => http.Response(
          jsonEncode({
            'success': true,
            'usage': {
              'fetchedAt': '2026-11-13T00:00:00Z',
              'accounts': [
                {
                  'accountId': 'usage-id',
                  'provider': 'gemini',
                  'label': 'Gemini',
                  'status': 'error',
                  'reason': 'rate-limited',
                  'planName': 'Plus',
                  'fetchedAt': '2026-11-12T00:00:00Z',
                  'windows': [
                    {
                      'kind': 'model',
                      'modelName': 'Gemini Pro',
                      'usedPercent': 42.5,
                      'resetsAt': '2026-11-14T00:00:00Z',
                    },
                  ],
                },
              ],
            },
          }),
          200,
        ),
      ),
    );
    addTearDown(client.close);
    final usage = await AiAccountsRepository(client).fetchUsage(pc);
    expect(usage.accounts.single.reason, 'rate-limited');
    expect(usage.accounts.single.windows.single.usedPercent, 42.5);
    expect(usage.accounts.single.windows.single.modelName, 'Gemini Pro');
    expect(usage.accounts.single.planName, 'Plus');
    expect(usage.fetchedAt, DateTime.utc(2026, 11, 13));
  });
  test('깨진 응답은 빈 계정으로 위장하지 않고 실패한다', () async {
    final client = DesktopClient(
      httpClient: MockClient(
        (_) async => http.Response('{"success":true,"accounts":{}}', 200),
      ),
    );
    addTearDown(client.close);
    await expectLater(
      AiAccountsRepository(client).fetchAccounts(pc),
      throwsFormatException,
    );
  });
}
