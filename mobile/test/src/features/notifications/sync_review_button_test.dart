import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/features/notifications/domain/inbox_notification.dart';
import 'package:kanvibe_mobile/src/features/notifications/presentation/background_sync_card.dart';

import '../../../support/fake_desktop.dart';
import '../../../support/fake_notification_inbox.dart';
import 'sync_review_test.dart' show reviewPayload;

Map<String, dynamic> _syncStatus() => {
  'status': {
    'phase': 'completed',
    'isRunning': false,
    'completedStages': 3,
    'reviewNeeded': true,
    'boardUpdated': true,
    'error': null,
  },
};

void main() {
  testWidgets('sync 알림이 없으면 탐지 0개를 표시하고 검토를 조회하지 않는다', (tester) async {
    final client = InboxDesktopClient();
    var reviews = 0;
    client.onJsonRequest = (_, segments, _, _) async {
      if (segments.first == 'notifications') reviews++;
      return _syncStatus();
    };
    await tester.pumpWidget(
      wrapWithApp(
        const Scaffold(body: BackgroundSyncCard()),
        client: client,
        size: phoneSize,
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('탐지 항목 0개'), findsOneWidget);
    expect(reviews, 0);
  });

  testWidgets('탐지 버튼은 기존 검토창을 열고 닫힌 동안 정리한 결과를 재개한다', (tester) async {
    const size = Size(900, 1800);
    tester.view.physicalSize = size;
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final pending = Completer<Map<String, dynamic>>();
    final readPending = Completer<void>();
    final client = InboxDesktopClient();
    client.onRead = () => readPending.future;
    client.notificationsByConnection['127.0.0.1:9736'] = [
      InboxNotification(
        id: 'notice',
        title: '동기화 결과',
        body: '',
        taskId: null,
        isRead: false,
        createdAt: DateTime.utc(2026, 11, 14),
        actionType: 'background-sync-review',
      ),
    ];
    var deletes = 0;
    client.onJsonRequest = (method, segments, _, _) async {
      if (segments.first == 'sync') return _syncStatus();
      if (method == 'POST') {
        if (++deletes == 1) return pending.future;
        return {
          'success': true,
          'deletedTaskIds': ['two'],
        };
      }
      return {
        'success': true,
        'review': {...(reviewPayload()['review'] as Map), 'failures': []},
      };
    };
    await tester.pumpWidget(
      wrapWithApp(
        const Scaffold(body: BackgroundSyncCard()),
        client: client,
        size: size,
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.text('탐지 항목 4개'));
    await tester.tap(find.text('탐지 항목 4개'));
    await tester.pumpAndSettle();
    expect(find.byType(Dialog), findsOneWidget);
    expect(client.readCalls, hasLength(1));
    expect(find.text('동기화 결과 검토'), findsOneWidget);
    await tester.tap(find.text('Merged one'));
    await tester.tap(find.text('Merged two'));
    await tester.pump();
    await tester.tap(find.widgetWithText(OutlinedButton, '선택 항목 삭제'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(TextButton, '선택 항목 삭제'));
    await tester.pump();
    expect(deletes, 1);
    await tester.pumpAndSettle();
    expect(find.byType(Dialog), findsNothing);
    expect(find.text('정리 중'), findsOneWidget);
    expect(find.text('백그라운드에서 정리합니다. 검토 창을 닫아도 계속 진행됩니다.'), findsOneWidget);
    await tester.tap(find.text('정리 중'));
    await tester.pumpAndSettle();
    expect(find.byType(Dialog), findsOneWidget);
    expect(find.text('삭제하는 중…'), findsOneWidget);
    await tester.tap(find.byTooltip('닫기'));
    await tester.pumpAndSettle();
    pending.complete({
      'success': true,
      'deletedTaskIds': ['one'],
    });
    await tester.pumpAndSettle();
    expect(find.text('탐지 항목 2개'), findsOneWidget);
    await tester.tap(find.text('탐지 항목 2개'));
    await tester.pumpAndSettle();
    expect(find.text('2개 태스크를 삭제했습니다.'), findsOneWidget);
    expect(deletes, 2);
    expect(client.readCalls, hasLength(1));
    readPending.complete();
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
  });
}
