import 'dart:convert';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../data/diff_repository.dart';
import '../domain/git_diff.dart';

final diffRepositoryProvider = Provider<DiffRepository>(
  (ref) => DiffRepository(ref.watch(desktopClientProvider)),
);
final diffFilesProvider = FutureProvider.autoDispose
    .family<List<ChangedFile>, String>((ref, taskId) async {
      final connection = (await ref.watch(connectionControllerProvider.future))
          .activeConnection;
      if (connection == null) throw StateError('연결된 PC가 없습니다.');
      return ref.read(diffRepositoryProvider).fetchFiles(connection, taskId);
    }, retry: (_, _) => null);
final diffStatsProvider =
    FutureProvider.autoDispose<Map<String, TaskChangeStats>>((ref) async {
      final connection = (await ref.watch(connectionControllerProvider.future))
          .activeConnection;
      if (connection == null) throw StateError('연결된 PC가 없습니다.');
      return ref.read(diffRepositoryProvider).fetchStats(connection);
    }, retry: (_, _) => null);

typedef DiffFileTarget = ({String taskId, String filePath});
final diffDocumentProvider = AsyncNotifierProvider.autoDispose
    .family<DiffDocumentController, DiffFileContents, DiffFileTarget>(
      DiffDocumentController.new,
      retry: (_, _) => null,
    );

class DiffDocumentController extends AsyncNotifier<DiffFileContents> {
  DiffDocumentController(this.target);
  final DiffFileTarget target;
  DesktopConnection? _connection;
  bool _saving = false;
  int _generation = 0;

  @override
  Future<DiffFileContents> build() async {
    _generation++;
    final connection = (await ref.watch(connectionControllerProvider.future))
        .activeConnection;
    if (connection == null) throw StateError('연결된 PC가 없습니다.');
    _connection = connection;
    return ref
        .read(diffRepositoryProvider)
        .fetchFile(connection, target.taskId, target.filePath);
  }

  Future<bool> save(String content) async {
    final connection = _connection;
    final current = state.value;
    if (_saving || current == null || connection == null) return false;
    final selected = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    if (selected?.id != connection.id || selected?.token != connection.token) {
      return false;
    }
    if (utf8.encode(content).length > 1024 * 1024 ||
        content.contains('\u0000')) {
      throw ArgumentError('파일이 너무 크거나 텍스트 파일이 아닙니다.');
    }
    final generation = _generation;
    _saving = true;
    try {
      await ref
          .read(diffRepositoryProvider)
          .save(connection, target.taskId, target.filePath, content);
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
      state = AsyncData(
        DiffFileContents(original: current.original, content: content),
      );
      ref.invalidate(diffFilesProvider(target.taskId));
      ref.invalidate(diffStatsProvider);
      return true;
    } finally {
      _saving = false;
    }
  }
}
