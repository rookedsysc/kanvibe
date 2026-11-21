import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/providers.dart';
import '../../../common/network/desktop_connection.dart';
import '../domain/board_view_options.dart';

final boardTaskKindProvider =
    NotifierProvider<BoardTaskKindController, BoardTaskKind>(
      BoardTaskKindController.new,
    );

class BoardTaskKindController extends Notifier<BoardTaskKind> {
  @override
  BoardTaskKind build() {
    ref.watch(
      connectionControllerProvider.select((value) => value.value?.activeId),
    );
    return BoardTaskKind.all;
  }

  void select(BoardTaskKind kind) => state = kind;
}

final boardSortProvider =
    AsyncNotifierProvider<BoardSortController, List<BoardSortKey>>(
      BoardSortController.new,
      retry: (_, _) => null,
    );

class BoardSortController extends AsyncNotifier<List<BoardSortKey>> {
  DesktopConnection? _connection;
  int _generation = 0;
  bool _isSaving = false;

  @override
  Future<List<BoardSortKey>> build() async {
    _generation++;
    final connection = (await ref.watch(connectionControllerProvider.future))
        .activeConnection;
    _connection = connection;
    if (connection == null) return [];
    final payload = await ref.read(desktopClientProvider).requestJson(
      connection,
      'GET',
      ['settings'],
    );
    final raw = (payload['settings'] as Map)['sortPreference'];
    final keys = raw is Map ? raw['keys'] : null;
    if (keys is! List) return [];
    final fields = <BoardSortField>{};
    return [
      for (final key in keys)
        if (key is Map)
          for (final field in BoardSortField.values)
            if (field.name == key['field'] && fields.add(field))
              BoardSortKey(field, descending: key['direction'] == 'desc'),
    ];
  }

  Future<bool> save(List<BoardSortKey> keys) async {
    final connection = _connection;
    final generation = _generation;
    if (connection == null || _isSaving) return false;
    _isSaving = true;
    try {
      await ref
          .read(desktopClientProvider)
          .requestJson(
            connection,
            'PATCH',
            ['settings'],
            body: {
              'sortPreference': {
                'keys': [for (final key in keys) key.toJson()],
              },
            },
          );
      if (!ref.mounted || generation != _generation) return false;
      state = AsyncData(List.unmodifiable(keys));
      return true;
    } catch (_) {
      return false;
    } finally {
      _isSaving = false;
    }
  }
}
