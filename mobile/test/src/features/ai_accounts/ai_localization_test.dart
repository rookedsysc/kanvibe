import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/constants/app_theme.dart';
import 'package:kanvibe_mobile/src/common/l10n/app_localizations.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/ai_accounts/presentation/ai_accounts_screen.dart';
import 'package:kanvibe_mobile/src/features/ai_sessions/presentation/live_ai_sessions_screen.dart';

import '../ai_sessions/ai_sessions_screens_test.dart' show live;
import 'ai_test_support.dart';

void main() {
  testWidgets('AI 계정은 PC 조회를 유지하면서 언어와 밝은 테마를 반영한다', (tester) async {
    var requests = 0;
    final client = AiStreamClient((_) async {
      requests++;
      return aiResponse({'success': true, 'accounts': []});
    });
    final container = ProviderContainer(
      overrides: [
        desktopClientProvider.overrideWithValue(client),
        connectionStoreProvider.overrideWithValue(AiMemoryConnections()),
      ],
    );
    final locale = ValueNotifier(const Locale('en'));
    await tester.pumpWidget(
      UncontrolledProviderScope(
        container: container,
        child: ValueListenableBuilder<Locale>(
          valueListenable: locale,
          builder: (_, value, _) => MaterialApp(
            locale: value,
            supportedLocales: AppLocalizations.supportedLocales,
            localizationsDelegates: const [
              AppLocalizations.delegate,
              GlobalMaterialLocalizations.delegate,
              GlobalWidgetsLocalizations.delegate,
              GlobalCupertinoLocalizations.delegate,
            ],
            theme: buildAppTheme(Brightness.light),
            home: const AiAccountsScreen(),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('AI accounts'), findsOneWidget);
    expect(find.byTooltip('Refresh'), findsOneWidget);
    expect(
      Theme.of(tester.element(find.byType(Scaffold))).brightness,
      Brightness.light,
    );
    locale.value = const Locale('zh');
    await tester.pumpAndSettle();
    expect(find.text('AI 账户'), findsOneWidget);
    expect(find.byTooltip('刷新'), findsOneWidget);
    expect(requests, 1);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    container.dispose();
    client.close();
    locale.dispose();
  });

  testWidgets('터미널 선택 콜백이 이동한 화면을 세션 화면이 다시 pop하지 않는다', (tester) async {
    final client = AiStreamClient((_) async => aiResponse(live()));
    String? selected;
    await mountAi(
      tester,
      Builder(
        builder: (context) => LiveAiSessionsScreen(
          taskId: 'task-1',
          onSelectWindow: (id) {
            selected = id;
            Navigator.of(context).push(
              MaterialPageRoute<void>(
                builder: (_) =>
                    const Scaffold(body: Text('Terminal destination')),
              ),
            );
          },
        ),
      ),
      client,
    );
    await tester.tap(find.text('터미널 열기 · Agent'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));
    expect(selected, '@3');
    expect(find.text('Terminal destination'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
}
