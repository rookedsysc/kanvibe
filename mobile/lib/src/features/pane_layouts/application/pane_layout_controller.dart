import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/providers.dart';
import '../../../common/network/desktop_connection.dart';
import '../data/pane_layout_repository.dart';
import '../domain/pane_layout.dart';

final paneLayoutRepositoryProvider = Provider<PaneLayoutRepository>(
  (ref) => PaneLayoutRepository(ref.watch(desktopClientProvider)),
);
final paneLayoutsProvider =
    AsyncNotifierProvider.autoDispose<PaneLayoutsController, List<PaneLayout>>(
      PaneLayoutsController.new,
      retry: (_, _) => null,
    );

class PaneLayoutsController extends AsyncNotifier<List<PaneLayout>> {
  bool _saving = false;
  int _generation = 0;
  DesktopConnection? _connection;
  @override
  Future<List<PaneLayout>> build() async {
    _generation++;
    final connection = (await ref.watch(connectionControllerProvider.future))
        .activeConnection;
    if (connection == null) throw StateError('연결된 PC가 없습니다.');
    _connection = connection;
    return ref.read(paneLayoutRepositoryProvider).fetch(connection);
  }

  Future<bool> _mutate(Future<void> Function(DesktopConnection) action) async {
    final connection = _connection;
    final generation = _generation;
    if (_saving || connection == null) return false;
    final selected = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    if (selected?.id != connection.id || selected?.token != connection.token) {
      return false;
    }
    _saving = true;
    try {
      await action(connection);
      if (!ref.mounted) return false;
      final active = ref
          .read(connectionControllerProvider)
          .value
          ?.activeConnection;
      if (generation != _generation ||
          active?.id != connection.id ||
          active?.token != connection.token) {
        return false;
      }
      ref.invalidateSelf();
      return true;
    } finally {
      _saving = false;
    }
  }

  Future<bool> save(
    PaneLayoutKind kind,
    List<String> commands,
    String? projectId,
  ) => _mutate(
    (connection) => ref
        .read(paneLayoutRepositoryProvider)
        .save(connection, kind, commands, projectId),
  );
  Future<bool> delete(String id) => _mutate(
    (connection) =>
        ref.read(paneLayoutRepositoryProvider).delete(connection, id),
  );
}
