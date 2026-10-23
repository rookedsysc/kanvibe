import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/app.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/board/presentation/board_screen.dart';
import 'package:kanvibe_mobile/src/features/connection/presentation/connection_screen.dart';
import 'package:kanvibe_mobile/src/features/connection/presentation/desktop_connections_screen.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';

import '../support/fake_desktop.dart';

Future<FakeConnectionStore> pumpApp(
  WidgetTester tester, {
  required FakeDesktopClient client,
  FakeConnectionStore? initialStore,
  Size size = phoneSize,
}) async {
  await tester.binding.setSurfaceSize(size);
  final store = initialStore ?? FakeConnectionStore();

  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        desktopClientProvider.overrideWithValue(client),
        connectionStoreProvider.overrideWithValue(store),
      ],
      child: const KanvibeMobileApp(),
    ),
  );
  await tester.pumpAndSettle();
  return store;
}

void main() {
  const pcA = DesktopConnection(
    host: '127.0.0.1',
    port: 9736,
    token: 'token-a',
    name: '작업실',
  );
  const pcB = DesktopConnection(
    host: '127.0.0.2',
    port: 19736,
    token: 'token-b',
    name: '집 PC',
  );
  final twoPcs = SavedConnections(connections: [pcA, pcB], activeId: pcA.id);
  final boards = {
    pcA.host: {
      'tasks': {
        'todo': [
          {'id': 'a', 'title': 'A의 태스크', 'status': 'todo'},
        ],
      },
    },
    pcB.host: {
      'tasks': {
        'todo': [
          {'id': 'b', 'title': 'B의 태스크', 'status': 'todo'},
        ],
      },
    },
  };

  for (final size in [phoneSize, tabletSize]) {
    testWidgets('${size.width}dp에서 PC 전환 후 해당 보드를 표시하고 다른 토큰은 보존한다', (
      tester,
    ) async {
      final client = FakeDesktopClient(boardsByHost: boards);
      final store = await pumpApp(
        tester,
        client: client,
        initialStore: FakeConnectionStore(initial: twoPcs),
        size: size,
      );
      expect(find.text('A의 태스크'), findsOneWidget);

      await tester.tap(find.text('작업실'));
      await tester.pumpAndSettle();
      expect(find.byType(DesktopConnectionsScreen), findsOneWidget);
      await tester.tap(find.text('집 PC'));
      await tester.pumpAndSettle();

      expect(find.text('B의 태스크'), findsOneWidget);
      expect((await store.readRegistry()).connections, hasLength(2));
      expect((await store.readRegistry()).activeConnection?.id, pcB.id);
      expect(client.unpairCalls, isEmpty);
    });
  }

  testWidgets('PC 추가 폼으로 새 PC를 등록한 뒤 새 보드로 이동한다', (tester) async {
    final store = await pumpApp(
      tester,
      client: FakeDesktopClient(pairToken: 'token-b', boardsByHost: boards),
      initialStore: FakeConnectionStore(
        initial: SavedConnections(connections: [pcA], activeId: pcA.id),
      ),
    );
    await tester.tap(find.text('작업실'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('PC 추가'));
    await tester.pumpAndSettle();

    final fields = find.byType(TextFormField);
    await tester.enterText(fields.at(0), '집 PC');
    await tester.enterText(fields.at(1), pcB.host);
    await tester.enterText(fields.at(2), '${pcB.port}');
    await tester.enterText(fields.at(3), '123456');
    await tester.ensureVisible(find.text('연결'));
    await tester.tap(find.text('연결'));
    await tester.pumpAndSettle();

    expect(find.text('B의 태스크'), findsOneWidget);
    expect((await store.readRegistry()).connections, hasLength(2));
    expect((await store.readRegistry()).activeConnection?.name, '집 PC');
  });

  testWidgets('현재 PC를 삭제하면 남은 PC를 직접 선택할 수 있다', (tester) async {
    final client = FakeDesktopClient(boardsByHost: boards);
    final store = await pumpApp(
      tester,
      client: client,
      initialStore: FakeConnectionStore(initial: twoPcs),
    );
    await tester.tap(find.text('작업실'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('작업실 삭제'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('삭제').last);
    await tester.pumpAndSettle();

    expect(find.byType(DesktopConnectionsScreen), findsOneWidget);
    expect(find.text('작업할 PC를 선택해 주세요.'), findsOneWidget);
    expect(client.unpairCalls.single.id, pcA.id);
    expect((await store.readRegistry()).connections, hasLength(1));
    await tester.tap(find.text('집 PC'));
    await tester.pumpAndSettle();
    expect(find.text('B의 태스크'), findsOneWidget);
  });

  testWidgets('PC가 꺼져 있어도 목록에서는 삭제하고 원격 해제 안내를 보여 준다', (tester) async {
    final store = await pumpApp(
      tester,
      client: FakeDesktopClient(
        unpairError: const DesktopRequestException(503),
      ),
    );
    await tester.tap(find.text('127.0.0.1'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('127.0.0.1 삭제'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('삭제').last);
    await tester.pumpAndSettle();

    expect((await store.readRegistry()).connections, isEmpty);
    expect(find.byType(ConnectionScreen), findsOneWidget);
    expect(
      find.textContaining('PC의 KanVibe 설정에서도 이 기기를 삭제해 주세요'),
      findsOneWidget,
    );
  });

  testWidgets('현재 PC의 401은 다른 PC의 저장 정보를 남기고 선택 화면으로 보낸다', (tester) async {
    final store = await pumpApp(
      tester,
      client: FakeDesktopClient(boardError: const DesktopRequestException(401)),
      initialStore: FakeConnectionStore(initial: twoPcs),
    );

    expect(find.byType(DesktopConnectionsScreen), findsOneWidget);
    expect((await store.readRegistry()).connections.map((pc) => pc.id), [
      pcB.id,
    ]);
    expect((await store.readRegistry()).activeConnection, isNull);
  });

  testWidgets('데스크탑이 토큰을 거절하면 페어링 화면으로 되돌린다', (tester) async {
    final client = FakeDesktopClient(
      boardError: const DesktopRequestException(401),
    );
    final store = await pumpApp(tester, client: client);

    /// 데스크탑에서 기기 연결을 끊으면 남은 토큰으로는 아무것도 못 한다.
    /// 네트워크를 확인하라는 안내만 남으면 다시 붙을 길이 없다.
    expect(find.byType(ConnectionScreen), findsOneWidget);
    expect(find.byType(BoardScreen), findsNothing);
    expect(
      (await store.readRegistry()).connections,
      isEmpty,
      reason: '기기에 남은 토큰까지 지워야 다음 실행에서도 페어링 화면으로 열린다',
    );
  });

  testWidgets('토큰 문제가 아닌 실패는 보드에서 다시 시도하게 둔다', (tester) async {
    final store = await pumpApp(
      tester,
      client: FakeDesktopClient(boardError: const DesktopRequestException(503)),
    );

    expect(find.byType(BoardScreen), findsOneWidget);
    expect(find.text('다시 시도'), findsOneWidget);
    expect(
      (await store.readRegistry()).connections,
      isNotEmpty,
      reason: '데스크탑이 잠깐 못 받은 것뿐이면 페어링을 날릴 이유가 없다',
    );
  });

  testWidgets('이미 거절된 토큰은 데스크탑을 부르지 않고 기기에서만 지운다', (tester) async {
    final client = FakeDesktopClient(
      boardError: const DesktopRequestException(401),
    );
    final store = await pumpApp(tester, client: client);

    /// 사용자가 누른 끊기와 달리, 여기서는 데스크탑이 이미 이 토큰을 물린 뒤다.
    /// 그 토큰으로 기기 삭제를 요청해 봐야 401만 한 번 더 받고 두 뜻이 뒤섞인다.
    expect(client.unpairCalls, isEmpty);
    expect((await store.readRegistry()).connections, isEmpty);
  });
}
