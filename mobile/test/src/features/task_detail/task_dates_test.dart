import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/features/board/domain/board_task.dart';
import 'package:kanvibe_mobile/src/features/board/domain/task_status.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/widgets/task_info_sheet.dart';

void main() {
  testWidgets('task info displays both dates and omits missing dates', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: TaskInfoSheet(
            task: BoardTask(
              id: 'dates',
              title: 'Task dates',
              status: TaskStatus.todo,
              createdAt: DateTime.utc(2025, 1, 15, 13, 20),
              updatedAt: DateTime.utc(2026, 8, 22, 16, 40),
            ),
          ),
        ),
      ),
    );
    expect(find.text('생성 시각'), findsOneWidget);
    expect(find.text('수정 시각'), findsOneWidget);
    expect(find.textContaining('2025'), findsOneWidget);
    expect(find.textContaining('2026'), findsOneWidget);
    await tester.pumpWidget(
      const MaterialApp(
        home: Scaffold(
          body: TaskInfoSheet(
            task: BoardTask(
              id: 'legacy',
              title: 'Legacy task',
              status: TaskStatus.todo,
            ),
          ),
        ),
      ),
    );
    expect(find.text('생성 시각'), findsNothing);
    expect(find.text('수정 시각'), findsNothing);
    expect(tester.takeException(), isNull);
  });
}
