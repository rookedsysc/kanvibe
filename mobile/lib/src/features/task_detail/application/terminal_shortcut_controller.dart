import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../task_create/domain/session_type.dart';
import '../data/terminal_shortcut_store.dart';
import '../domain/terminal_shortcut.dart';

final terminalShortcutStoreProvider = Provider<TerminalShortcutStore>(
  (ref) => TerminalShortcutStore(),
);

final terminalShortcutControllerProvider =
    AsyncNotifierProvider<
      TerminalShortcutController,
      Map<SessionType, TerminalShortcutProfile>
    >(TerminalShortcutController.new, retry: (_, _) => null);

class TerminalShortcutController
    extends AsyncNotifier<Map<SessionType, TerminalShortcutProfile>> {
  Future<void> _pendingSave = Future.value();

  @override
  Future<Map<SessionType, TerminalShortcutProfile>> build() =>
      ref.read(terminalShortcutStoreProvider).read();

  Future<void> save(SessionType type, TerminalShortcutProfile profile) {
    final result = _pendingSave.then((_) async {
      final current = await future;
      final updated = {...current, type: profile};
      await ref.read(terminalShortcutStoreProvider).write(updated);
      if (ref.mounted) state = AsyncData(updated);
    });
    _pendingSave = result.then<void>(
      (_) {},
      onError: (Object error, StackTrace stack) {},
    );
    return result;
  }
}
