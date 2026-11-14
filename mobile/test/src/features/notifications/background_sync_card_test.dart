import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/notifications/presentation/background_sync_card.dart';

import '../../../support/fake_desktop.dart';

Map<String, dynamic> _response(String phase) => {
  'status': {
    'phase': phase,
    'isRunning': phase == 'pulls',
    'completedStages': phase == 'pulls'
        ? 2
        : phase == 'completed'
        ? 3
        : 0,
    'reviewNeeded': false,
    'boardUpdated': false,
    'error': null,
  },
};

void main() {
  testWidgets('연결된 PC가 없으면 시작 버튼을 잠그고 연결 상태를 안내한다', (tester) async {
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          desktopClientProvider.overrideWithValue(FakeDesktopClient()),
          connectionStoreProvider.overrideWithValue(
            FakeConnectionStore(initial: const SavedConnections()),
          ),
        ],
        child: const MaterialApp(home: Scaffold(body: BackgroundSyncCard())),
      ),
    );
    await tester.pumpAndSettle();
    expect(
      tester.widget<OutlinedButton>(find.byType(OutlinedButton)).onPressed,
      isNull,
    );
    expect(find.text('연결된 PC가 없습니다.'), findsOneWidget);
  });

  testWidgets('알림 진행 카드는 실제 단계와 잠긴 실행 버튼을 표시한다', (tester) async {
    final client = FakeDesktopClient();
    client.onJsonRequest = (_, _, _, _) async => _response('pulls');
    await tester.pumpWidget(
      wrapWithApp(
        const Scaffold(body: BackgroundSyncCard()),
        client: client,
        size: phoneSize,
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('변경 가져오는 중'), findsOneWidget);
    expect(find.text('2 / 3 단계 완료'), findsOneWidget);
    expect(
      tester.widget<OutlinedButton>(find.byType(OutlinedButton)).onPressed,
      isNull,
    );
    expect(
      tester
          .widget<LinearProgressIndicator>(find.byType(LinearProgressIndicator))
          .value,
      closeTo(2 / 3, 0.001),
    );
  });

  testWidgets('단계 갱신을 기다리는 동안 이전 진행 표시를 유지한다', (tester) async {
    final client = FakeDesktopClient();
    final refresh = Completer<Map<String, dynamic>>();
    var fetches = 0;
    client.onJsonRequest = (_, _, _, _) async =>
        fetches++ == 0 ? _response('pulls') : refresh.future;
    await tester.pumpWidget(
      wrapWithApp(
        const Scaffold(body: BackgroundSyncCard()),
        client: client,
        size: phoneSize,
      ),
    );
    await tester.pumpAndSettle();
    client.openedEvents['127.0.0.1:9736']!.add({
      'type': 'background-sync-status-changed',
    });
    await tester.pump();
    await tester.pump();
    expect(find.text('변경 가져오는 중'), findsOneWidget);
    expect(find.text('동기화 상태 확인 중…'), findsNothing);
    refresh.complete(_response('completed'));
    await tester.pumpAndSettle();
    expect(find.text('동기화가 끝났습니다. 새 변경 사항은 없습니다.'), findsOneWidget);
  });

  testWidgets('시작 응답을 기다리는 동안 중복 탭을 막고 변경 없음 결과를 남긴다', (tester) async {
    final client = FakeDesktopClient();
    final pending = Completer<Map<String, dynamic>>();
    var starts = 0;
    client.onJsonRequest = (method, _, _, _) async {
      if (method == 'POST') {
        starts++;
        return pending.future;
      }
      return _response('idle');
    };
    await tester.pumpWidget(
      wrapWithApp(
        const Scaffold(body: BackgroundSyncCard()),
        client: client,
        size: phoneSize,
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.text('지금 동기화'));
    await tester.pump();
    expect(
      tester.widget<OutlinedButton>(find.byType(OutlinedButton)).onPressed,
      isNull,
    );
    expect(starts, 1);
    pending.complete(_response('completed'));
    await tester.pumpAndSettle();
    expect(find.text('동기화가 끝났습니다. 새 변경 사항은 없습니다.'), findsOneWidget);
    expect(
      tester.widget<OutlinedButton>(find.byType(OutlinedButton)).onPressed,
      isNotNull,
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets('다른 PC 조회 중 이전 PC의 오류를 표시하지 않는다', (tester) async {
    const pcA = DesktopConnection(host: 'a', port: 9736, token: 'a');
    const pcB = DesktopConnection(host: 'b', port: 9736, token: 'b');
    final client = FakeDesktopClient();
    final pending = Completer<Map<String, dynamic>>();
    var calls = 0;
    client.onJsonRequest = (_, _, _, _) async {
      if (calls++ == 0) {
        throw const DesktopRequestException(500, message: 'PC A offline');
      }
      return pending.future;
    };
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          desktopClientProvider.overrideWithValue(client),
          connectionStoreProvider.overrideWithValue(
            FakeConnectionStore(
              initial: SavedConnections(
                connections: [pcA, pcB],
                activeId: pcA.id,
              ),
            ),
          ),
        ],
        child: const MaterialApp(home: Scaffold(body: BackgroundSyncCard())),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('PC A offline'), findsOneWidget);
    final container = ProviderScope.containerOf(
      tester.element(find.byType(BackgroundSyncCard)),
    );
    await container.read(connectionControllerProvider.notifier).select(pcB.id);
    await tester.pump();
    await tester.pump();
    expect(find.text('PC A offline'), findsNothing);
    expect(find.text('동기화 상태 확인 중…'), findsOneWidget);
    pending.complete(_response('completed'));
    await tester.pumpAndSettle();
  });
}
