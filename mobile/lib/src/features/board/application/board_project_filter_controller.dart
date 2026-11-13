import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/providers.dart';
import '../data/board_snapshot.dart';
import '../data/board_project_filter_store.dart';

final boardProjectFilterStoreProvider = Provider<BoardProjectFilterStore>(
  (ref) => BoardProjectFilterStore(),
);

final boardProjectFilterProvider =
    NotifierProvider.family<BoardProjectFilterController, Set<String>, String>(
      BoardProjectFilterController.new,
    );

/// 기기 저장소가 실패해도 보드는 메모리의 선택으로 계속 사용할 수 있다.
class BoardProjectFilterController extends Notifier<Set<String>> {
  BoardProjectFilterController(this._connectionId);

  final String _connectionId;
  int _revision = 0;
  Future<void> _pendingSave = Future.value();

  @override
  Set<String> build() {
    _restore(++_revision);
    return const {};
  }

  Future<void> _restore(int revision) async {
    try {
      final selected = await ref
          .read(boardProjectFilterStoreProvider)
          .read(_connectionId);
      if (ref.mounted && revision == _revision) state = selected;
    } catch (error) {
      debugPrint('프로젝트 필터 복원 실패: $error');
    }
  }

  Future<void> _save(Set<String> selected) {
    ++_revision;
    state = selected;
    final store = ref.read(boardProjectFilterStoreProvider);
    _pendingSave = _pendingSave
        .then((_) => store.write(_connectionId, selected))
        .catchError((Object error) {
          debugPrint('프로젝트 필터 저장 실패: $error');
        });
    return _pendingSave;
  }

  Future<void> toggleProject(String projectId) {
    final selected = {...state};
    if (!selected.remove(projectId)) selected.add(projectId);
    return _save(Set.unmodifiable(selected));
  }

  Future<void> showAllProjects() => _save(const {});
}

final filteredBoardProvider = Provider<AsyncValue<BoardSnapshot>>((ref) {
  final board = ref.watch(boardProvider);
  final connectionId = ref.watch(
    connectionControllerProvider.select(
      (value) => value.value?.activeConnection?.id,
    ),
  );
  if (connectionId == null) return board;
  final selected = ref.watch(boardProjectFilterProvider(connectionId));
  return board.whenData((snapshot) => snapshot.filterByProjectIds(selected));
});
