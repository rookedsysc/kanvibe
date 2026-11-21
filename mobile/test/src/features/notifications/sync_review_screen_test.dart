import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/features/notifications/presentation/sync_review_screen.dart';

import '../ai_accounts/ai_test_support.dart';
import 'sync_review_test.dart' show reviewPayload;

void main() {
  testWidgets('네 종류 결과를 표시하고 선택한 태스크만 확인 후 삭제한다', (tester) async {
    tester.view.physicalSize = const Size(900, 1800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    Map<String, dynamic>? sent;
    final client = AiStreamClient((request) async {
      if (request.method == 'POST') {
        sent = jsonDecode(request.body) as Map<String, dynamic>;
        return aiResponse({
          'success': true,
          'deletedTaskIds': ['two'],
        });
      }
      return aiResponse(reviewPayload());
    });
    await mountAi(
      tester,
      const SyncReviewScreen(notificationId: 'notice'),
      client,
    );
    for (final label in [
      '새로 등록된 Worktree',
      '자동 Pull 결과',
      '동기화 실패',
      '병합된 PR 태스크',
    ]) {
      expect(find.text(label), findsOneWidget);
    }
    final delete = find.widgetWithText(OutlinedButton, '선택 항목 삭제');
    expect(tester.widget<OutlinedButton>(delete).onPressed, isNull);
    await tester.tap(find.text('Merged two'));
    await tester.pump();
    await tester.tap(delete);
    await tester.pumpAndSettle();
    expect(sent, isNull);
    expect(find.textContaining('태스크 1개와'), findsOneWidget);
    await tester.tap(find.widgetWithText(TextButton, '선택 항목 삭제'));
    await tester.pumpAndSettle();
    expect(sent, {
      'taskIds': ['two'],
    });
    expect(find.text('1개 태스크를 삭제했습니다.'), findsOneWidget);
    final tiles = tester
        .widgetList<CheckboxListTile>(find.byType(CheckboxListTile))
        .toList();
    expect(tiles.first.onChanged, isNotNull);
    expect(tiles.last.onChanged, isNull);
    expect(tester.takeException(), isNull);
  });

  testWidgets('검토 조회 실패에서 재시도하고 빈 상태를 표시한다', (tester) async {
    var fail = true;
    final client = AiStreamClient(
      (request) async => fail
          ? aiResponse({
              'success': false,
              'error': 'review unavailable',
            }, status: 503)
          : aiResponse({'success': true, 'review': {}}),
    );
    await mountAi(
      tester,
      const SyncReviewScreen(notificationId: 'notice'),
      client,
    );
    expect(find.text('review unavailable'), findsOneWidget);
    fail = false;
    await tester.tap(find.text('다시 시도'));
    await tester.pumpAndSettle();
    expect(find.text('검토할 변경 사항이 없습니다.'), findsOneWidget);
  });
}
