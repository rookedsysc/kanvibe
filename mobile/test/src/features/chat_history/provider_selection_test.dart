import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/features/chat_history/domain/chat_history.dart';
import 'package:kanvibe_mobile/src/features/chat_history/presentation/chat_history_screen.dart';

import '../../../support/fake_desktop.dart';

class ProviderHistoryClient extends FakeDesktopClient {
  @override
  Future<ChatHistoryPage> fetchChatHistory(
    DesktopConnection connection,
    String taskId, {
    String? query,
    String? cursor,
  }) async => const ChatHistoryPage(
    providers: ['claude', 'codex', 'gemini'],
    sessions: [
      ChatSession(id: 'c', provider: 'claude', title: 'Claude conversation'),
      ChatSession(id: 'o', provider: 'codex', title: 'Codex conversation'),
      ChatSession(id: 'g', provider: 'gemini', title: 'Gemini conversation'),
    ],
  );
}

void main() {
  testWidgets(
    'provider chips select several providers and All restores every conversation',
    (tester) async {
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            desktopClientProvider.overrideWithValue(ProviderHistoryClient()),
            connectionStoreProvider.overrideWithValue(FakeConnectionStore()),
          ],
          child: const MaterialApp(home: ChatHistoryScreen(taskId: 'task')),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(FilterChip, 'claude'));
      await tester.pumpAndSettle();
      expect(find.text('Claude conversation'), findsOneWidget);
      expect(find.text('Codex conversation'), findsNothing);
      await tester.tap(find.widgetWithText(FilterChip, 'codex'));
      await tester.pumpAndSettle();
      expect(find.text('Claude conversation'), findsOneWidget);
      expect(find.text('Codex conversation'), findsOneWidget);
      expect(find.text('Gemini conversation'), findsNothing);
      await tester.tap(find.widgetWithText(FilterChip, '전체'));
      await tester.pumpAndSettle();
      expect(find.text('Gemini conversation'), findsOneWidget);
    },
  );
}
