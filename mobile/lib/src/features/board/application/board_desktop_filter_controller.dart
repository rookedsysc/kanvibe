import 'package:flutter_riverpod/flutter_riverpod.dart';

final boardDesktopFilterProvider =
    NotifierProvider.family<BoardDesktopFilterController, Set<String>, String>(
      BoardDesktopFilterController.new,
    );

class BoardDesktopFilterController extends Notifier<Set<String>> {
  BoardDesktopFilterController(String connectionId);

  @override
  Set<String> build() => const {};

  void toggleDesktop(String desktop) {
    final selected = {...state};
    if (!selected.remove(desktop)) selected.add(desktop);
    state = Set.unmodifiable(selected);
  }

  void showAllDesktops() => state = const {};
}
