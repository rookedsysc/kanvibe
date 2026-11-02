import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/providers.dart';
import '../data/board_snapshot.dart';

final boardProjectFilterProvider =
    NotifierProvider.family<BoardProjectFilterController, Set<String>, String>(
      BoardProjectFilterController.new,
    );

class BoardProjectFilterController extends Notifier<Set<String>> {
  BoardProjectFilterController(String connectionId);

  @override
  Set<String> build() => const {};

  void toggleProject(String projectId) {
    final selected = {...state};
    if (!selected.remove(projectId)) selected.add(projectId);
    state = Set.unmodifiable(selected);
  }

  void showAllProjects() => state = const {};
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
