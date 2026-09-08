import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/app.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/board/presentation/board_screen.dart';
import 'package:kanvibe_mobile/src/features/connection/presentation/connection_screen.dart';

import '../support/fake_desktop.dart';

Future<FakeConnectionStore> pumpApp(
  WidgetTester tester, {
  required Object boardError,
}) async {
  await tester.binding.setSurfaceSize(phoneSize);
  final store = FakeConnectionStore();

  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        desktopClientProvider.overrideWithValue(
          FakeDesktopClient(boardError: boardError),
        ),
        connectionStoreProvider.overrideWithValue(store),
      ],
      child: const KanvibeMobileApp(),
    ),
  );
  await tester.pumpAndSettle();
  return store;
}

void main() {
  testWidgets('데스크탑이 토큰을 거절하면 페어링 화면으로 되돌린다', (tester) async {
    final store = await pumpApp(
      tester,
      boardError: const DesktopRequestException(401),
    );

    /// 데스크탑에서 기기 연결을 끊으면 남은 토큰으로는 아무것도 못 한다.
    /// 네트워크를 확인하라는 안내만 남으면 다시 붙을 길이 없다.
    expect(find.byType(ConnectionScreen), findsOneWidget);
    expect(find.byType(BoardScreen), findsNothing);
    expect(
      await store.read(),
      isNull,
      reason: '기기에 남은 토큰까지 지워야 다음 실행에서도 페어링 화면으로 열린다',
    );
  });

  testWidgets('토큰 문제가 아닌 실패는 보드에서 다시 시도하게 둔다', (tester) async {
    final store = await pumpApp(
      tester,
      boardError: const DesktopRequestException(503),
    );

    expect(find.byType(BoardScreen), findsOneWidget);
    expect(find.text('다시 시도'), findsOneWidget);
    expect(
      await store.read(),
      isNotNull,
      reason: '데스크탑이 잠깐 못 받은 것뿐이면 페어링을 날릴 이유가 없다',
    );
  });
}
