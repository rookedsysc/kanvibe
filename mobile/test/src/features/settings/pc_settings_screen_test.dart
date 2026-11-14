import 'dart:convert';
import 'dart:async';

import 'package:http/http.dart' as http;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/features/settings/presentation/pc_settings_screen.dart';

import '../ai_accounts/ai_test_support.dart';
import 'pc_settings_controller_test.dart' show pcSettings;

class _PcSettingsClient extends AiStreamClient {
  _PcSettingsClient(Future<http.Response> Function(http.Request) handler)
    : super((request) async {
        if (request.method == 'GET' &&
            request.url.path.endsWith('/sync/status')) {
          return aiResponse({
            'success': true,
            'status': {
              'phase': 'idle',
              'isRunning': false,
              'completedStages': 0,
              'reviewNeeded': false,
              'boardUpdated': false,
              'error': null,
            },
          });
        }
        return handler(request);
      });

  @override
  DesktopEventStream openEventStream(DesktopConnection connection) {
    final events = StreamController<Map<String, dynamic>>();
    return DesktopEventStream(
      events: events.stream,
      onClose: events.close,
      readCloseCode: () => null,
    );
  }
}

void main() {
  testWidgets('초기 설정 대기와 조회 실패 재시도를 표시한다', (tester) async {
    final pending = Completer<http.Response>();
    var first = true;
    final client = _PcSettingsClient((request) async {
      if (request.url.path.endsWith('change-targets')) {
        return aiResponse({
          'success': true,
          'targets': {'taskCount': 0, 'liveSessionCount': 0, 'sshHosts': []},
        });
      }
      if (first) return pending.future;
      return aiResponse(pcSettings());
    });
    await mountAi(tester, const PcSettingsScreen(), client);
    expect(find.byType(CircularProgressIndicator), findsOneWidget);
    pending.complete(
      aiResponse({'success': false, 'error': 'settings offline'}, status: 503),
    );
    await tester.pumpAndSettle();
    expect(find.text('settings offline'), findsOneWidget);
    first = false;
    await tester.tap(find.text('다시 시도'));
    await tester.pumpAndSettle();
    expect(find.text('새 태스크 기본 세션'), findsOneWidget);
  });

  testWidgets('PC 기본 세션과 유효한 주기를 저장하고 범위 밖 주기는 보내지 않는다', (tester) async {
    final writes = <Map<String, dynamic>>[];
    final client = _PcSettingsClient((request) async {
      if (request.url.path.endsWith('change-targets')) {
        return aiResponse({
          'success': true,
          'targets': {'taskCount': 0, 'liveSessionCount': 0, 'sshHosts': []},
        });
      }
      if (request.method == 'PATCH') {
        final body = jsonDecode(request.body) as Map<String, dynamic>;
        writes.add(body);
        return aiResponse(
          pcSettings(
            type: body['defaultSessionType'] as String? ?? 'herdr',
            minutes: 5,
          ),
        );
      }
      return aiResponse(pcSettings());
    });
    await mountAi(tester, const PcSettingsScreen(), client);
    final dropdown = find
        .byWidgetPredicate((widget) => widget is DropdownButtonFormField)
        .first;
    await tester.tap(dropdown);
    await tester.pumpAndSettle();
    await tester.tap(find.text('tmux').last);
    await tester.pumpAndSettle();
    expect(writes.single, {'defaultSessionType': 'tmux'});
    final interval = find.byType(TextFormField);
    await tester.enterText(interval, '0');
    await tester.pump();
    await tester.tap(find.text('주기 저장'));
    await tester.pump();
    expect(find.text('1분에서 1440분 사이로 입력해 주세요.'), findsOneWidget);
    expect(writes.length, 1);
    await tester.enterText(interval, '5');
    await tester.pump();
    await tester.tap(find.text('주기 저장'));
    await tester.pumpAndSettle();
    expect(writes.last, {
      'backgroundSync': {'isEnabled': true, 'intervalMs': 300000},
    });
    expect(tester.takeException(), isNull);
  });

  testWidgets('일괄 변경은 종료 선택과 대상 개수를 확인한 다음 실행한다', (tester) async {
    tester.view.physicalSize = const Size(900, 1800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    Map<String, dynamic>? sent;
    final client = _PcSettingsClient((request) async {
      if (request.url.path.endsWith('change-targets')) {
        return aiResponse({
          'success': true,
          'targets': {
            'taskCount': 2,
            'liveSessionCount': 3,
            'sshHosts': [null, 'work'],
          },
        });
      }
      if (request.url.path.endsWith('change-all')) {
        sent = jsonDecode(request.body) as Map<String, dynamic>;
        return aiResponse({
          'success': true,
          'result': {'changedCount': 1, 'failedCount': 1},
        });
      }
      return aiResponse(pcSettings());
    });
    await mountAi(tester, const PcSettingsScreen(), client);
    await tester.tap(find.text('기존 세션 종료'));
    await tester.tap(find.text('변경 내용 확인'));
    await tester.pumpAndSettle();
    expect(sent, isNull);
    expect(find.textContaining('2개 태스크를 herdr'), findsOneWidget);
    expect(find.textContaining('종료합니다'), findsOneWidget);
    await tester.tap(find.text('일괄 변경'));
    await tester.pumpAndSettle();
    expect(sent, {'sessionType': 'herdr', 'shouldCloseExistingSessions': true});
    expect(find.text('1개 변경 · 1개 실패'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('수동 동기화 실패 후 재시도 결과를 표시한다', (tester) async {
    tester.view.physicalSize = const Size(900, 1800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    var fail = true;
    final client = _PcSettingsClient((request) async {
      if (request.method == 'POST') {
        return fail
            ? aiResponse({
                'success': false,
                'error': 'sync failed',
              }, status: 500)
            : aiResponse({
                'success': true,
                'status': {
                  'phase': 'completed',
                  'isRunning': false,
                  'completedStages': 3,
                  'reviewNeeded': true,
                  'boardUpdated': true,
                  'error': null,
                },
              });
      }
      if (request.url.path.endsWith('/sync/status')) {
        return aiResponse({
          'success': true,
          'status': {
            'phase': 'idle',
            'isRunning': false,
            'completedStages': 0,
            'reviewNeeded': false,
            'boardUpdated': false,
            'error': null,
          },
        });
      }
      if (request.url.path.endsWith('change-targets')) {
        return aiResponse({
          'success': true,
          'targets': {'taskCount': 0, 'liveSessionCount': 0, 'sshHosts': []},
        });
      }
      return aiResponse(pcSettings());
    });
    await mountAi(tester, const PcSettingsScreen(), client);
    await tester.tap(find.text('지금 동기화'));
    await tester.pumpAndSettle();
    expect(find.text('sync failed'), findsOneWidget);
    fail = false;
    await tester.tap(find.text('다시 시도'));
    await tester.pumpAndSettle();
    expect(find.text('동기화가 끝났습니다. 알림함에서 검토할 결과를 확인해 주세요.'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
}
