import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/features/ai_sessions/presentation/live_ai_sessions_screen.dart';

import '../ai_accounts/ai_test_support.dart';

Map<String, dynamic> live() => {
  'success': true,
  'liveSessions': {
    'taskId': 'task-1',
    'isRemote': false,
    'sessions': [
      {
        'provider': 'claude',
        'sessionId': 's1',
        'currentTask': 'Fix mobile',
        'state': 'running',
        'runningSubtasks': [
          {'id': 'sub', 'name': 'Explore'},
        ],
        'terminalWindow': {'windowId': '@3', 'windowName': 'Agent'},
      },
    ],
  },
};
void main() {
  testWidgets('실행 목록·하위 작업과 window 선택을 표시한다', (tester) async {
    String? window;
    final client = AiStreamClient((_) async => aiResponse(live()));
    await mountAi(
      tester,
      LiveAiSessionsScreen(
        taskId: 'task-1',
        onSelectWindow: (value) => window = value,
      ),
      client,
    );
    expect(find.text('Fix mobile'), findsOneWidget);
    expect(find.text('Explore'), findsOneWidget);
    expect(find.text('1개 세션 · 1개 실행 중'), findsOneWidget);
    await tester.tap(find.text('터미널 열기 · Agent'));
    expect(window, '@3');
  });
  testWidgets('호출 그래프는 중첩 Agent와 시간 막대를 표시한다', (tester) async {
    final client = AiStreamClient(
      (request) async => aiResponse(
        request.url.path.endsWith('live-ai-sessions')
            ? live()
            : {
                'success': true,
                'graph': {
                  'provider': 'claude',
                  'sessionId': 's1',
                  'startedAt': '2026-11-13T00:00:00Z',
                  'readAt': '2026-11-13T00:01:00Z',
                  'roots': [
                    {
                      'id': 'root',
                      'agentType': 'worker',
                      'task': 'Root task',
                      'startedAt': '2026-11-13T00:00:00Z',
                      'endedAt': null,
                      'children': [
                        {
                          'id': 'child',
                          'agentType': 'Explore',
                          'task': 'Nested task',
                          'skill': 'search',
                          'startedAt': '2026-11-13T00:00:10Z',
                          'endedAt': '2026-11-13T00:00:30Z',
                          'children': [],
                        },
                      ],
                    },
                  ],
                },
              },
      ),
    );
    await mountAi(tester, const LiveAiSessionsScreen(taskId: 'task-1'), client);
    await tester.tap(find.text('호출 그래프'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));
    expect(find.text('Agent 호출 그래프'), findsOneWidget);
    expect(find.text('Nested task'), findsOneWidget);
    expect(find.text('Skill · search'), findsOneWidget);
    expect(find.text('완료 · 20초'), findsOneWidget);
  });
  testWidgets('빈 세션 목록과 새로고침을 제공한다', (tester) async {
    var requests = 0;
    final client = AiStreamClient((_) async {
      requests++;
      return aiResponse({
        'success': true,
        'liveSessions': {'taskId': 'task-1', 'isRemote': false, 'sessions': []},
      });
    });
    await mountAi(tester, const LiveAiSessionsScreen(taskId: 'task-1'), client);
    expect(find.textContaining('AI 세션이 없습니다'), findsOneWidget);
    await tester.tap(find.byTooltip('새로고침'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    expect(requests, 2);
  });
  testWidgets('백그라운드에서는 폴링을 멈추고 복귀 시 갱신한다', (tester) async {
    var requests = 0;
    final client = AiStreamClient((_) async {
      requests++;
      return aiResponse(live());
    });
    await mountAi(tester, const LiveAiSessionsScreen(taskId: 'task-1'), client);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
    await tester.pump(const Duration(seconds: 4));
    expect(requests, 1);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    expect(requests, 2);
  });
}
