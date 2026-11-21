import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/providers.dart';
import '../../../common/network/desktop_client.dart';
import '../domain/board_task.dart';

final taskSearchProvider = FutureProvider<List<BoardTask>>((ref) async {
  final connection = (await ref.watch(connectionControllerProvider.future))
      .activeConnection;
  if (connection == null) return [];
  try {
    final payload = await ref.read(desktopClientProvider).requestJson(
      connection,
      'GET',
      ['tasks', 'search'],
    );
    return [
      for (final raw in payload['tasks'] as List)
        ?BoardTask.fromJson(Map<String, dynamic>.from(raw as Map), const {}),
    ];
  } on DesktopRequestException catch (error) {
    if (error.isUnauthorized && ref.mounted) {
      await ref
          .read(connectionControllerProvider.notifier)
          .forgetLocally(connection.id, connection.token);
    }
    rethrow;
  }
}, retry: (_, _) => null);
