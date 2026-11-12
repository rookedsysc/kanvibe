import 'package:flutter_riverpod/flutter_riverpod.dart';

final boardDesktopFilterProvider =
    NotifierProvider.family<BoardDesktopFilterController, String, String>(
      BoardDesktopFilterController.new,
    );

class BoardDesktopFilterController extends Notifier<String> {
  BoardDesktopFilterController(String connectionId);

  @override
  String build() => '';

  void select(String desktop) => state = desktop;
}
