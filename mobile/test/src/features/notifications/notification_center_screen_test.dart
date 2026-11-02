import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/constants/app_theme.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/board/presentation/board_screen.dart';
import 'package:kanvibe_mobile/src/features/notifications/presentation/notification_center_screen.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/task_detail_screen.dart';

import '../../../support/fake_desktop.dart';
import '../../../support/fake_notification_inbox.dart';

const _connectionId = '127.0.0.1:9736';

Future<void> _openInbox(WidgetTester tester, InboxDesktopClient client) async {
  await tester.binding.setSurfaceSize(phoneSize);
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await tester.pumpWidget(
    wrapWithApp(const BoardScreen(), client: client, size: phoneSize),
  );
  await tester.pumpAndSettle();
  await tester.tap(find.byTooltip('알림'));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('보드의 알림 버튼에서 데스크톱 알림함을 연다', (tester) async {
    await tester.binding.setSurfaceSize(phoneSize);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await tester.pumpWidget(
      wrapWithApp(
        const BoardScreen(),
        client: FakeDesktopClient(),
        size: phoneSize,
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byTooltip('알림'), findsOneWidget);
    await tester.tap(find.byTooltip('알림'));
    await tester.pumpAndSettle();
    expect(find.text('알림이 없습니다.'), findsOneWidget);
    expect(find.text('안 읽음'), findsOneWidget);
  });

  testWidgets('미읽음 배지와 안 읽음 필터는 데스크톱 목록에서 계산한다', (tester) async {
    final client = InboxDesktopClient();
    client.notificationsByConnection[_connectionId] = [
      inboxNotice(),
      inboxNotice(id: 'read', isRead: true),
    ];
    await _openInbox(tester, client);
    await tester.tap(find.text('안 읽음'));
    await tester.pumpAndSettle();
    expect(find.text('알림 n1'), findsOneWidget);
    expect(find.text('알림 read'), findsNothing);
    await tester.tap(find.byTooltip('읽음 처리'));
    await tester.pumpAndSettle();
    expect(find.text('읽지 않은 알림이 없습니다.'), findsOneWidget);
    expect(client.readCalls.single.id, 'n1');
    await tester.pageBack();
    await tester.pumpAndSettle();
    final badge = tester.widget<Badge>(
      find.descendant(of: find.byTooltip('알림'), matching: find.byType(Badge)),
    );
    expect(badge.isLabelVisible, isFalse);
  });

  testWidgets('알림 선택은 최신 태스크를 조회하고 읽음 처리한 뒤 연다', (tester) async {
    final client = InboxDesktopClient(
      tasksById: const {
        'task-1': {'id': 'task-1', 'title': '갱신된 태스크 제목', 'status': 'review'},
      },
    );
    client.notificationsByConnection[_connectionId] = [inboxNotice()];
    await _openInbox(tester, client);
    await tester.tap(find.text('알림 n1'));
    await tester.pumpAndSettle();
    expect(find.byType(TaskDetailScreen), findsOneWidget);
    expect(find.text('갱신된 태스크 제목'), findsWidgets);
    expect(client.fetchedTasks, ['task-1']);
    expect(
      client.notificationsByConnection[_connectionId]!.single.isRead,
      isTrue,
    );
    await tester.pageBack();
    await tester.pumpAndSettle();
    expect(find.text('알림 n1'), findsOneWidget);
  });

  testWidgets('삭제된 태스크는 안내하고 알림함에 남는다', (tester) async {
    final client = InboxDesktopClient();
    client.notificationsByConnection[_connectionId] = [inboxNotice()];
    await _openInbox(tester, client);
    await tester.tap(find.text('알림 n1'));
    await tester.pumpAndSettle();
    expect(find.byType(TaskDetailScreen), findsNothing);
    expect(find.text('태스크가 삭제되었거나 더 이상 존재하지 않습니다.'), findsOneWidget);
    expect(find.text('알림 n1'), findsOneWidget);
  });

  testWidgets('태스크 없는 알림도 내용을 확인하고 모두 읽음 처리한다', (tester) async {
    final client = InboxDesktopClient();
    client.notificationsByConnection[_connectionId] = [
      inboxNotice(taskId: null),
      inboxNotice(id: 'n2'),
    ];
    await _openInbox(tester, client);
    expect(find.text('데스크톱에서 확인'), findsOneWidget);
    await tester.tap(find.text('알림 n1'));
    await tester.pumpAndSettle();
    expect(find.byType(TaskDetailScreen), findsNothing);
    expect(client.fetchedTasks, isEmpty);
    await tester.tap(find.byTooltip('모두 읽음'));
    await tester.pumpAndSettle();
    expect(client.readAllCalls, [_connectionId]);
    expect(
      client.notificationsByConnection[_connectionId]!.every(
        (entry) => entry.isRead,
      ),
      isTrue,
    );
  });

  testWidgets('조회 실패 뒤 다시 시도하면 알림을 표시한다', (tester) async {
    final client = InboxDesktopClient();
    client.notificationsByConnection[_connectionId] = [inboxNotice()];
    client.onFetch = (_) async => throw const DesktopRequestException(500);
    await _openInbox(tester, client);
    expect(find.textContaining('알림을 불러오지 못했습니다'), findsOneWidget);
    client.onFetch = null;
    await tester.tap(find.text('다시 시도'));
    await tester.pumpAndSettle();
    expect(find.text('알림 n1'), findsOneWidget);
  });

  testWidgets('읽음 실패 시 미읽음을 유지하고 안내한다', (tester) async {
    final client = InboxDesktopClient();
    client.notificationsByConnection[_connectionId] = [inboxNotice()];
    client.onRead = () async => throw const DesktopRequestException(500);
    await _openInbox(tester, client);
    await tester.tap(find.byTooltip('읽음 처리'));
    await tester.pumpAndSettle();
    expect(find.text('읽음 처리하지 못했습니다. 다시 시도해 주세요.'), findsOneWidget);
    expect(find.byTooltip('읽음 처리'), findsOneWidget);
    expect(
      client.notificationsByConnection[_connectionId]!.single.isRead,
      isFalse,
    );
  });

  for (final size in [const Size(375, 812), const Size(812, 375), tabletSize]) {
    testWidgets('알림함은 $size에서 큰 글자와 긴 내용으로도 넘치지 않는다', (tester) async {
      await tester.binding.setSurfaceSize(size);
      addTearDown(() => tester.binding.setSurfaceSize(null));
      final client = InboxDesktopClient();
      client.notificationsByConnection[_connectionId] = [
        inboxNotice(id: '긴 알림 제목 ' * 15),
      ];
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            desktopClientProvider.overrideWithValue(client),
            connectionStoreProvider.overrideWithValue(FakeConnectionStore()),
          ],
          child: MaterialApp(
            theme: buildAppTheme(),
            builder: (context, child) => MediaQuery(
              data: MediaQuery.of(context).copyWith(
                textScaler: TextScaler.linear(2),
                disableAnimations: true,
              ),
              child: child!,
            ),
            home: const NotificationCenterScreen(),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      for (final tooltip in ['모두 읽음', '새로고침']) {
        final size = tester.getSize(find.byTooltip(tooltip));
        expect(size.width, greaterThanOrEqualTo(48));
        expect(size.height, greaterThanOrEqualTo(48));
      }
    });
  }
}
