import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/ai_sessions/application/running_ai_panes.dart';

import '../../../support/fake_desktop.dart';

class SnapshotClient extends FakeDesktopClient {
  final calls = <String>[];
  @override
  Future<Map<String, dynamic>> requestJson(
    DesktopConnection connection,
    String method,
    List<String> segments, {
    Map<String, String>? query,
    Map<String, dynamic>? body,
    Duration timeout = const Duration(seconds: 30),
  }) async {
    calls.add(connection.host);
    expect(segments, ['ai', 'running-panes']);
    return {
      'panes': [
        {
          'provider': 'codex',
          'worktreePath': '/${connection.host}',
          'windowId': '@1',
        },
      ],
    };
  }
}

void main() {
  testWidgets(
    'all cards share one foreground request and switching PC replaces the snapshot',
    (tester) async {
      const a = DesktopConnection(host: 'a', port: 9736, token: 'a');
      const b = DesktopConnection(host: 'b', port: 9736, token: 'b');
      final client = SnapshotClient();
      final calls = client.calls;
      final container = ProviderContainer(
        overrides: [
          desktopClientProvider.overrideWithValue(client),
          connectionStoreProvider.overrideWithValue(
            FakeConnectionStore(
              initial: SavedConnections(connections: [a, b], activeId: a.id),
            ),
          ),
        ],
      );
      await container.read(connectionControllerProvider.future);
      final first = container.listen(runningAiPanesProvider, (_, _) {});
      final second = container.listen(runningAiPanesProvider, (_, _) {});
      await tester.pump();
      await tester.pump();
      expect(calls, ['a']);
      expect(
        container.read(runningAiPanesProvider).requireValue.single.worktreePath,
        '/a',
      );
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
      await tester.pump(const Duration(seconds: 15));
      expect(calls, ['a']);
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
      await tester.pump();
      expect(calls, ['a', 'a']);
      await container.read(connectionControllerProvider.notifier).select(b.id);
      container.read(runningAiPanesProvider);
      await tester.pump(const Duration(milliseconds: 1));
      expect(
        container.read(connectionControllerProvider).requireValue.activeId,
        b.id,
      );
      await tester.pump(const Duration(seconds: 5));
      expect(calls.last, 'b');
      expect(
        container.read(runningAiPanesProvider).requireValue.single.worktreePath,
        '/b',
      );
      first.close();
      second.close();
      container.dispose();
      client.close();
      await tester.pump();
    },
  );
}
