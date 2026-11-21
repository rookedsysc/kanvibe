import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/ai_accounts/domain/ai_account.dart';
import 'package:kanvibe_mobile/src/features/ai_accounts/presentation/ai_accounts_screen.dart';
import 'package:kanvibe_mobile/src/features/ai_accounts/presentation/ai_usage_screen.dart';
import 'package:kanvibe_mobile/src/features/ai_accounts/presentation/ai_account_login_screen.dart';
import 'package:xterm/xterm.dart' as xterm;

import 'ai_test_support.dart';

const loginAccount = AiAccount(
  accountId: 'opaque-id',
  provider: AiAccountProvider.claude,
  label: 'Work',
  isLoggedIn: false,
  isRemovable: true,
);
void main() {
  setUp(() => FlutterSecureStorage.setMockInitialValues({}));
  testWidgets('계정 로딩과 기본 계정의 삭제 보호를 표시한다', (tester) async {
    final pending = Completer<http.Response>();
    final client = AiStreamClient((_) => pending.future);
    await mountAi(tester, const AiAccountsScreen(), client);
    expect(find.byType(CircularProgressIndicator), findsOneWidget);
    pending.complete(
      aiResponse({
        'success': true,
        'accounts': [fixtureAccount(removable: false)],
      }),
    );
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    expect(find.text('Work Account'), findsOneWidget);
    expect(find.text('계정 삭제'), findsNothing);
    expect(find.text('로그인 필요'), findsOneWidget);
  });
  testWidgets('계정 오류를 재시도하고 빈 결과를 구분한다', (tester) async {
    var fail = true;
    final client = AiStreamClient(
      (_) async => fail
          ? aiResponse({'success': false, 'error': 'PC offline'}, status: 503)
          : aiResponse({'success': true, 'accounts': []}),
    );
    await mountAi(tester, const AiAccountsScreen(), client);
    expect(find.text('PC offline'), findsOneWidget);
    fail = false;
    await tester.tap(find.text('다시 시도'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    expect(find.textContaining('등록된 AI 계정이 없습니다'), findsOneWidget);
  });
  testWidgets('추가 폼은 경로를 거절하고 유효한 이름으로 로그인에 진입한다', (tester) async {
    final requests = <http.Request>[];
    final client = AiStreamClient((request) async {
      requests.add(request);
      return aiResponse(
        request.method == 'POST'
            ? {'success': true, 'accountId': 'created-id'}
            : {'success': true, 'accounts': []},
      );
    });
    await mountAi(tester, const AiAccountsScreen(), client);
    await tester.tap(find.text('계정 추가'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 200));
    await tester.enterText(find.byType(TextFormField), '../escape');
    await tester.tap(find.text('추가하고 로그인'));
    await tester.pump();
    expect(find.textContaining('이름을 확인해 주세요'), findsOneWidget);
    expect(requests.where((request) => request.method == 'POST'), isEmpty);
    await tester.enterText(find.byType(TextFormField), 'team');
    await tester.tap(find.text('추가하고 로그인'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));
    expect(client.loginSegments, ['ai', 'accounts', 'created-id', 'login']);
    expect(find.text('Claude 로그인'), findsOneWidget);
  });
  testWidgets('계정 삭제는 확인 전 쓰지 않고 성공 후 목록을 갱신한다', (tester) async {
    var removed = false;
    var deletes = 0;
    final client = AiStreamClient((request) async {
      if (request.method == 'DELETE') {
        deletes++;
        removed = true;
        return aiResponse({'success': true, 'outcome': 'ok'});
      }
      return aiResponse({
        'success': true,
        'accounts': removed ? [] : [fixtureAccount()],
      });
    });
    await mountAi(tester, const AiAccountsScreen(), client);
    await tester.tap(find.text('계정 삭제'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 200));
    expect(deletes, 0);
    await tester.tap(find.text('삭제'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 200));
    expect(deletes, 1);
    expect(find.text('Work Account'), findsNothing);
  });
  testWidgets('사용량은 기간·구독·마지막 값·실패 사유·초기화 시각을 표시한다', (tester) async {
    final client = AiStreamClient(
      (_) async => aiResponse({
        'success': true,
        'usage': {
          'fetchedAt': '2026-11-13T00:00:00Z',
          'accounts': [
            {
              'accountId': 'usage-id',
              'provider': 'claude',
              'label': 'Work usage',
              'status': 'error',
              'reason': 'rate-limited',
              'planName': 'Pro',
              'fetchedAt': '2026-11-12T00:00:00Z',
              'windows': [
                {
                  'kind': 'weekly',
                  'usedPercent': 42.5,
                  'modelName': null,
                  'resetsAt': '2026-11-14T00:00:00Z',
                },
              ],
            },
          ],
        },
      }),
    );
    await mountAi(tester, const AiUsageScreen(), client);
    expect(find.text('Work usage'), findsOneWidget);
    expect(find.text('주간 한도 · 42.5% 사용'), findsOneWidget);
    expect(find.textContaining('마지막으로 확인한 값'), findsOneWidget);
    expect(find.textContaining('요청이 제한'), findsOneWidget);
    expect(find.textContaining('초기화 '), findsOneWidget);
  });
  testWidgets('로그인 JSON 출력·인증 응답·복사·크기와 종료를 연결한다', (tester) async {
    final client = AiStreamClient(
      (_) async => aiResponse({'success': true, 'accounts': []}),
    );
    String? copied;
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      (call) async {
        if (call.method == 'Clipboard.setData') {
          copied = (call.arguments as Map)['text'] as String;
        }
        return null;
      },
    );
    addTearDown(
      () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        null,
      ),
    );
    await mountAi(
      tester,
      const AiAccountLoginScreen(account: loginAccount, connection: fixturePc),
      client,
    );
    client.frames.add('{"type":"ready"}');
    client.frames.add(
      '{"type":"output","data":"Verification code: fixture-code\\r\\nhttps://example.test/auth\\r\\n"}',
    );
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    final terminal = tester
        .widget<xterm.TerminalView>(find.byType(xterm.TerminalView))
        .terminal;
    expect(terminal.buffer.getText(), contains('fixture-code'));
    expect(find.byTooltip('로그인 링크 열기'), findsOneWidget);
    expect(find.text('https://example.test/auth'), findsOneWidget);
    expect(client.sizes, isNotEmpty);
    await tester.enterText(find.byType(TextField), 'reply-code');
    await tester.tap(find.byTooltip('인증 응답 전송'));
    await tester.pump();
    expect(client.writes, contains('reply-code\r'));
    await tester.tap(find.byTooltip('출력 복사'));
    await tester.pump();
    expect(copied, contains('fixture-code'));
    client.frames.add('{"type":"exit","exitCode":0}');
    await tester.pump();
    expect(find.textContaining('계정 목록에서 로그인 상태'), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
    expect(client.closes, 1);
  });
  testWidgets('완성된 로그인 URL만 열고 잘못된 프레임은 정리한다', (tester) async {
    final client = AiStreamClient(
      (_) async => aiResponse({'success': true, 'accounts': []}),
    );
    String? launched;
    const channel = MethodChannel('plugins.flutter.io/url_launcher');
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(channel, (
      call,
    ) async {
      launched = (call.arguments as Map)['url'] as String?;
      return true;
    });
    addTearDown(
      () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        channel,
        null,
      ),
    );
    await mountAi(
      tester,
      const AiAccountLoginScreen(account: loginAccount, connection: fixturePc),
      client,
    );
    client.frames.add('{"type":"ready"}');
    client.frames.add('{"type":"output","data":"https://example.test/auth"}');
    await tester.pump();
    expect(find.byTooltip('로그인 링크 열기'), findsNothing);
    client.frames.add('{"type":"output","data":"?code=fixture\\r\\n"}');
    await tester.pump();
    expect(find.text('https://example.test/auth?code=fixture'), findsOneWidget);
    await tester.tap(find.byTooltip('로그인 링크 열기'));
    await tester.pump();
    expect(launched, 'https://example.test/auth?code=fixture');
    client.frames.add('{broken');
    await tester.pump();
    expect(client.closes, 1);
    expect(find.textContaining('데스크탑 로그인 응답을 읽지 못했습니다'), findsOneWidget);
  });
  testWidgets('PC 전환은 로그인 소켓을 정리한다', (tester) async {
    final client = AiStreamClient(
      (_) async => aiResponse({'success': true, 'accounts': []}),
    );
    final container = await mountAi(
      tester,
      const AiAccountLoginScreen(account: loginAccount, connection: fixturePc),
      client,
    );
    await container
        .read(connectionControllerProvider.notifier)
        .select(otherPc.id);
    await tester.pump();
    expect(client.closes, 1);
    expect(find.textContaining('선택한 PC가 변경'), findsOneWidget);
    expect(find.text('다시 연결'), findsNothing);
    client.frames.add('{"type":"output","data":"late output"}');
    await tester.pump();
    expect(tester.takeException(), isNull);
  });
  testWidgets('OAuth 브라우저 전환 동안 유지하고 가로·큰 글자에서도 넘치지 않는다', (tester) async {
    tester.view.physicalSize = const Size(812, 375);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final client = AiStreamClient(
      (_) async => aiResponse({'success': true, 'accounts': []}),
    );
    await mountAi(
      tester,
      const AiAccountLoginScreen(account: loginAccount, connection: fixturePc),
      client,
      textScale: 2,
    );
    client.frames.add('{"type":"ready"}');
    await tester.pump();
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
    await tester.pump();
    expect(client.closes, 0);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pump();
    expect(tester.takeException(), isNull);
  });
}
