import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_socket_events.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/chat_history/domain/chat_history.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/pane_chat/application/pane_agent_watcher.dart';
import 'package:kanvibe_mobile/src/features/pane_chat/application/pane_chat_controller.dart';
import 'package:kanvibe_mobile/src/features/pane_chat/domain/pane_chat.dart';

import '../../../support/fake_desktop.dart';

const pc = DesktopConnection(host: 'a', port: 19746, token: 'token-a');
const target = (taskId: 'task-1', paneId: '%19');

/// 대화는 오래된 것부터 적는다. 실제 응답처럼 최신순으로 뒤집어 담고 마지막 줄의 순번을 싣는다
class _PushDesktop extends FakeDesktopClient {
  List<String> texts = ['첫 답변'];
  String epoch = 'epoch-a';
  String? provider = 'codex';
  String inputState = 'ready';
  final fetchedPanes = <String>[];

  @override
  Future<PaneChatSnapshot> fetchPaneChat(
    DesktopConnection connection,
    PaneChatTarget target,
  ) async {
    fetchedPanes.add(target.paneId);
    return PaneChatSnapshot.fromJson({
      'taskId': target.taskId,
      'paneId': target.paneId,
      'sessionType': 'tmux',
      'provider': provider,
      'targetToken': provider == null ? null : 'target-a',
      'canSend': provider != null,
      'inputState': inputState,
      'screen': '화면',
      'reason': null,
      'transcript': {
        'status': 'available',
        'reason': null,
        'sessionId': 'session-a',
        'messages': [
          for (final text in texts.reversed)
            {'role': 'assistant', 'fullText': text},
        ],
        'nextCursor': null,
        'epoch': epoch,
        'seq': texts.length,
      },
      'modelPicker': {'supported': true, 'mode': 'native-cli'},
    });
  }
}

/// 컨테이너가 남긴 타이머가 없는지까지 검사받도록 본문이 끝나면 바로 정리한다
void testPush(
  String description,
  Future<void> Function(WidgetTester tester) body,
) => testWidgets(description, (tester) async {
  await body(tester);
  for (final container in _containers) {
    container.dispose();
  }
  _containers.clear();
  await tester.pump();
});

final _containers = <ProviderContainer>[];

ProviderContainer containerFor(FakeDesktopClient client) {
  final container = ProviderContainer(
    overrides: [
      desktopClientProvider.overrideWithValue(client),
      connectionStoreProvider.overrideWithValue(
        FakeConnectionStore(
          initial: const SavedConnections(
            connections: [pc],
            activeId: 'a:19746',
          ),
        ),
      ),
    ],
  );
  _containers.add(container);
  return container;
}

Map<String, dynamic> rowsEvent(
  List<(int, String)> rows, {
  String epoch = 'epoch-a',
}) => {
  'type': 'pane-chat-rows',
  'taskId': target.taskId,
  'paneId': target.paneId,
  'epoch': epoch,
  'rows': [
    for (final (seq, text) in rows)
      {
        'seq': seq,
        'message': {'role': 'assistant', 'fullText': text},
      },
  ],
};

PaneChatTranscript transcriptOf(List<String> newestFirst, {int? seq}) =>
    PaneChatTranscript(
      available: true,
      messages: [
        for (final text in newestFirst)
          ChatMessage(role: 'assistant', text: text),
      ],
      epoch: 'epoch-a',
      seq: seq ?? newestFirst.length,
    );

List<String> textsOf(PaneChatTranscript transcript) =>
    transcript.messages.map((message) => message.text).toList();

void main() {
  group('밀려온 대화 줄의 반영', () {
    ChatMessage message(String text) =>
        ChatMessage(role: 'assistant', text: text);

    test('다음 순번의 줄은 최신 쪽에 붙이고 위치를 옮긴다', () {
      final result = transcriptOf(['b', 'a']).applyRows('epoch-a', [
        (seq: 3, message: message('c')),
        (seq: 4, message: message('d')),
      ]);
      expect(result.outcome, PaneChatRowsOutcome.applied);
      expect(textsOf(result.transcript), ['d', 'c', 'b', 'a']);
      expect(result.transcript.cursor, (epoch: 'epoch-a', seq: 4));
    });

    test('이미 가진 순번의 줄은 그 자리의 내용을 바꾼다', () {
      final result = transcriptOf(['답변을', 'a'])
          .applyRows('epoch-a', [(seq: 2, message: message('답변을 이어 씁니다'))]);
      expect(result.outcome, PaneChatRowsOutcome.applied);
      expect(textsOf(result.transcript), ['답변을 이어 씁니다', 'a']);
      expect(result.transcript.seq, 2);
    });

    test('순번을 건너뛴 줄은 반영하지 않고 다시 받아야 한다고 알린다', () {
      final transcript = transcriptOf(['b', 'a']);
      final result = transcript.applyRows('epoch-a', [
        (seq: 4, message: message('d')),
      ]);
      expect(result.outcome, PaneChatRowsOutcome.outOfSync);
      expect(result.transcript, same(transcript));
    });

    test('다른 epoch의 줄은 순번이 이어져 보여도 반영하지 않는다', () {
      final result = transcriptOf(['b', 'a'])
          .applyRows('epoch-b', [(seq: 3, message: message('c'))]);
      expect(result.outcome, PaneChatRowsOutcome.outOfSync);
    });

    test('창 밖으로 밀려난 오래된 순번의 줄은 달라진 것 없이 넘긴다', () {
      final result = transcriptOf([
        'z',
        'y',
      ], seq: 50).applyRows('epoch-a', [(seq: 10, message: message('old'))]);
      expect(result.outcome, PaneChatRowsOutcome.alreadyApplied);
    });

    test('순번을 매기지 않는 데스크탑의 대화에는 밀려온 줄을 붙이지 않는다', () {
      const transcript = PaneChatTranscript(available: true, messages: []);
      expect(transcript.cursor, isNull);
      expect(
        transcript.applyRows('epoch-a', [
          (seq: 1, message: message('a')),
        ]).outcome,
        PaneChatRowsOutcome.outOfSync,
      );
    });
  });

  group('push를 받는 채팅', () {
    late _PushDesktop client;
    late ProviderContainer container;
    late DesktopSocketEvents socket;

    Future<PaneChatSnapshot> startChat(WidgetTester tester) async {
      container.listen(paneChatProvider(target), (_, _) {});
      // 화면이 하듯 보내는 중인 메시지도 함께 듣는다
      container.listen(paneChatPendingPromptProvider(target), (_, _) {});
      await container.read(paneChatProvider(target).future);
      await tester.pump();
      return container.read(paneChatProvider(target)).requireValue.snapshot;
    }

    PaneChatSnapshot snapshot() =>
        container.read(paneChatProvider(target)).requireValue.snapshot;

    Future<void> push(WidgetTester tester, Map<String, dynamic> event) async {
      socket.add('a:19746', event);
      await tester.pump();
    }

    setUp(() {
      client = _PushDesktop()
        ..onPaneChatSubscription = (command) => {
          'ok': true,
          'panes': [
            for (final pane in command.fields['panes']! as List)
              {
                'taskId': (pane as Map)['taskId'],
                'paneId': pane['paneId'],
                'freshness': pane['cursor'] == null ? 'behind' : 'current',
              },
          ],
        };
      container = containerFor(client);
      socket = container.read(desktopSocketEventsProvider);
    });

    testPush('가진 위치를 실어 구독하고, 이후에는 조회 없이 밀려온 줄로 대화를 잇는다', (tester) async {
      await startChat(tester);
      final subscribed =
          client.paneChatSubscriptionCalls.last.fields['panes']! as List;
      expect(subscribed, [
        {
          'taskId': 'task-1',
          'paneId': '%19',
          'detail': 'chat',
          'wantsScreen': false,
          'cursor': {'epoch': 'epoch-a', 'seq': 1},
        },
      ]);
      expect(client.fetchedPanes, ['%19']);

      await push(tester, rowsEvent([(2, '둘째 답변')]));
      expect(textsOf(snapshot().transcript), ['둘째 답변', '첫 답변']);

      await tester.pump(const Duration(seconds: 10));
      expect(client.fetchedPanes, ['%19']);
    });

    testPush('순번을 건너뛴 줄이 오면 스냅샷을 한 번 다시 받는다', (tester) async {
      await startChat(tester);
      client.texts = ['첫 답변', '둘째 답변', '셋째 답변'];
      await push(tester, rowsEvent([(3, '셋째 답변')]));
      await tester.pump();
      expect(client.fetchedPanes, ['%19', '%19']);
      expect(textsOf(snapshot().transcript), ['셋째 답변', '둘째 답변', '첫 답변']);
    });

    testPush('기록이 교체됐다는 신호를 받으면 새 기록을 다시 받는다', (tester) async {
      await startChat(tester);
      client
        ..texts = ['새 세션의 답변']
        ..epoch = 'epoch-b';
      await push(tester, {
        'type': 'pane-chat-replaced',
        'taskId': 'task-1',
        'paneId': '%19',
        'epoch': 'epoch-b',
      });
      await tester.pump();
      expect(textsOf(snapshot().transcript), ['새 세션의 답변']);
      expect(snapshot().transcript.cursor, (epoch: 'epoch-b', seq: 1));
    });

    testPush('밀려온 상태는 실려 온 값만 바꾸고 대화는 그대로 둔다', (tester) async {
      await startChat(tester);
      await push(tester, {
        'type': 'pane-chat-state',
        'taskId': 'task-1',
        'paneId': '%19',
        'state': {'inputState': 'occupied'},
      });
      expect(snapshot().inputState, PaneChatInputState.occupied);
      expect(snapshot().screen, '화면');
      expect(textsOf(snapshot().transcript), ['첫 답변']);
      expect(client.fetchedPanes, ['%19']);
    });

    testPush('다른 pane의 이벤트는 이 채팅에 반영하지 않는다', (tester) async {
      await startChat(tester);
      await push(tester, {
        ...rowsEvent([(2, '남의 답변')]),
        'paneId': '%20',
      });
      expect(textsOf(snapshot().transcript), ['첫 답변']);
    });

    testPush('소켓이 끊기면 주기 조회로 돌아가고, 다시 붙으면 위치를 실어 구독을 다시 보낸다', (tester) async {
      await startChat(tester);
      await push(tester, const {'type': 'disconnected'});
      client.texts = ['첫 답변', '끊긴 동안의 답변'];
      await tester.pump(const Duration(seconds: 2));
      await tester.pump();
      expect(textsOf(snapshot().transcript), ['끊긴 동안의 답변', '첫 답변']);

      final subscriptionsBefore = client.paneChatSubscriptionCalls.length;
      await push(tester, const {'type': 'connected'});
      await tester.pump();
      expect(client.paneChatSubscriptionCalls.length, subscriptionsBefore + 1);
      expect(
        ((client.paneChatSubscriptionCalls.last.fields['panes']! as List).single
            as Map)['cursor'],
        {'epoch': 'epoch-a', 'seq': 2},
      );
      final fetchedBefore = client.fetchedPanes.length;
      await tester.pump(const Duration(seconds: 10));
      expect(client.fetchedPanes.length, fetchedBefore);
    });

    testPush('채팅을 보지 않게 되면 구독에서 빼고, CLI 화면이 보이면 화면까지 요청한다', (tester) async {
      await startChat(tester);
      final notifier = container.read(paneChatProvider(target).notifier);
      notifier.setWantsScreen(true);
      await tester.pump();
      expect(
        ((client.paneChatSubscriptionCalls.last.fields['panes']! as List).single
            as Map)['wantsScreen'],
        isTrue,
      );
      notifier.setActive(false);
      await tester.pump();
      expect(client.paneChatSubscriptionCalls.last.fields['panes'], isEmpty);
    });

    testPush('채팅 버튼용 구독은 에이전트가 켜지고 꺼지는 것을 조회 없이 따라간다', (tester) async {
      client.provider = null;
      container.listen(paneAgentProvider(target), (_, _) {});
      await tester.pump();
      await tester.pump();
      expect(
        ((client.paneChatSubscriptionCalls.last.fields['panes']! as List).single
            as Map)['detail'],
        'agent',
      );
      expect(container.read(paneAgentProvider(target)), isNull);
      final fetchedBefore = client.fetchedPanes.length;

      await push(tester, {
        'type': 'pane-chat-state',
        'taskId': 'task-1',
        'paneId': '%19',
        'state': {'provider': 'claude'},
      });
      expect(container.read(paneAgentProvider(target)), 'claude');
      await push(tester, {
        'type': 'pane-chat-state',
        'taskId': 'task-1',
        'paneId': '%19',
        'state': {'provider': null},
      });
      expect(container.read(paneAgentProvider(target)), isNull);
      await tester.pump(const Duration(seconds: 20));
      expect(client.fetchedPanes.length, fetchedBefore);
    });
  });

  testPush('구독을 모르는 데스크탑에서는 지금처럼 주기적으로 조회한다', (tester) async {
    final client = _PushDesktop();
    final container = containerFor(client);
    container.listen(paneChatProvider(target), (_, _) {});
    // 화면이 하듯 보내는 중인 메시지도 함께 듣는다
    container.listen(paneChatPendingPromptProvider(target), (_, _) {});
    await container.read(paneChatProvider(target).future);
    await tester.pump();
    client.texts = ['첫 답변', '둘째 답변'];
    await tester.pump(const Duration(seconds: 2));
    await tester.pump();
    expect(
      textsOf(
        container
            .read(paneChatProvider(target))
            .requireValue
            .snapshot
            .transcript,
      ),
      ['둘째 답변', '첫 답변'],
    );
    expect(client.fetchedPanes.length, greaterThanOrEqualTo(2));
  });
}
