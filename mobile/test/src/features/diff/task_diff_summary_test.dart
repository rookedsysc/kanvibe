import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/constants/app_colors.dart';
import 'package:kanvibe_mobile/src/common/constants/app_theme.dart';
import 'package:kanvibe_mobile/src/features/diff/application/diff_controller.dart';
import 'package:kanvibe_mobile/src/features/diff/domain/git_diff.dart';
import 'package:kanvibe_mobile/src/features/diff/presentation/diff_screen.dart';
import 'package:kanvibe_mobile/src/features/diff/presentation/widgets/task_diff_summary.dart';

void main() {
  for (final brightness in Brightness.values) {
    testWidgets(
      'compact counts use addition/deletion colors and open the task review: $brightness',
      (tester) async {
        await tester.pumpWidget(
          ProviderScope(
            overrides: [
              diffStatsProvider.overrideWith(
                (_) async => {
                  'task': const TaskChangeStats(
                    fileCount: 3,
                    additions: 12,
                    deletions: 4,
                  ),
                },
              ),
              diffFilesProvider('task').overrideWith((_) async => []),
            ],
            child: MaterialApp(
              theme: buildAppTheme(brightness),
              home: const Scaffold(
                body: TaskDiffSummary(taskId: 'task', compact: true),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        expect(find.text('3 파일  +12 −4'), findsOneWidget);
        final summary = tester.widget<Text>(find.text('3 파일  +12 −4'));
        expect(summary.textSpan, isNotNull);
        final spans = (summary.textSpan! as TextSpan).children!
            .cast<TextSpan>();
        expect(
          summary.style!.color,
          brightness == Brightness.dark
              ? AppPalette.dark.textMuted
              : AppPalette.light.textMuted,
        );
        expect(spans.map((span) => span.text), ['+12', ' −4']);
        final additions = spans.first.style!.color!;
        final deletions = spans.last.style!.color!;
        expect(additions.g, greaterThan(additions.r));
        expect(deletions.r, greaterThan(deletions.g));

        expect(find.byIcon(Icons.difference_outlined), findsNothing);
        expect(tester.getSize(find.byType(TextButton)).height, 32);
        await tester.tap(find.text('3 파일  +12 −4'));
        await tester.pumpAndSettle();
        expect(find.byType(DiffScreen), findsOneWidget);
        expect(find.text('변경된 파일이 없습니다.'), findsOneWidget);
      },
    );
  }
}
