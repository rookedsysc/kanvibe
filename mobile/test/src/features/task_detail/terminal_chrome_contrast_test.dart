import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/constants/app_colors.dart';
import 'package:kanvibe_mobile/src/common/constants/app_theme.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/board/domain/board_task.dart';
import 'package:kanvibe_mobile/src/features/board/domain/task_status.dart';
import 'package:kanvibe_mobile/src/features/task_detail/domain/mirror_pane.dart';
import 'package:kanvibe_mobile/src/features/task_detail/presentation/task_detail_screen.dart';

import '../../../support/fake_desktop.dart';

double contrast(Color foreground, Color background) {
  final a = foreground.computeLuminance();
  final b = background.computeLuminance();
  return ((a > b ? a : b) + 0.05) / ((a < b ? a : b) + 0.05);
}

void main() {
  for (final brightness in Brightness.values) {
    testWidgets(
      'terminal tab text stays readable on fixed dark chrome in $brightness',
      (tester) async {
        const pane = MirrorPane(
          id: '%1',
          tabId: '@1',
          tabName: 'Agent tab',
          command: 'zsh',
          left: 0,
          top: 0,
          width: 80,
          height: 24,
        );
        final client = FakeDesktopClient(
          surfaces: const TaskSurfaces(
            tabs: [
              MirrorTab(id: '@1', name: 'Agent tab', panes: [pane]),
            ],
          ),
        );
        await tester.pumpWidget(
          ProviderScope(
            overrides: [
              desktopClientProvider.overrideWithValue(client),
              connectionStoreProvider.overrideWithValue(FakeConnectionStore()),
            ],
            child: MaterialApp(
              theme: buildAppTheme(brightness),
              home: const TaskDetailScreen(
                task: BoardTask(
                  id: 'task',
                  title: 'Contrast task',
                  status: TaskStatus.progress,
                ),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        final title = tester.widget<Text>(find.text('zsh'));
        final detail = tester.widget<Text>(find.text('Agent tab %1'));
        expect(
          contrast(title.style!.color!, AppColors.terminalChrome),
          greaterThanOrEqualTo(4.5),
        );
        expect(
          contrast(detail.style!.color!, AppColors.terminalChrome),
          greaterThanOrEqualTo(4.5),
        );
        final add = tester.widget<Icon>(find.byIcon(Icons.add));
        expect(
          contrast(add.color!, AppColors.terminalChrome),
          greaterThanOrEqualTo(3),
        );
        client.openedPanes['%1']!.addError(StateError('offline'));
        await tester.pumpAndSettle();
        final failure = tester.widget<Text>(
          find.textContaining('네트워크를 확인해 주세요.'),
        );
        expect(
          contrast(failure.style!.color!, AppColors.terminalBg),
          greaterThanOrEqualTo(4.5),
        );
        await tester.pumpWidget(const SizedBox());
      },
    );
  }
}
