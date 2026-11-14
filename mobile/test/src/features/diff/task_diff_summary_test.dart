import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/features/diff/application/diff_controller.dart';
import 'package:kanvibe_mobile/src/features/diff/domain/git_diff.dart';
import 'package:kanvibe_mobile/src/features/diff/presentation/diff_screen.dart';
import 'package:kanvibe_mobile/src/features/diff/presentation/widgets/task_diff_summary.dart';

void main() {
  testWidgets('board summary shows cached counts and opens that task review', (
    tester,
  ) async {
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
        child: const MaterialApp(
          home: Scaffold(body: TaskDiffSummary(taskId: 'task', compact: true)),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('3 파일  +12 −4'), findsOneWidget);
    await tester.tap(find.text('3 파일  +12 −4'));
    await tester.pumpAndSettle();
    expect(find.byType(DiffScreen), findsOneWidget);
    expect(find.text('변경된 파일이 없습니다.'), findsOneWidget);
  });
}
