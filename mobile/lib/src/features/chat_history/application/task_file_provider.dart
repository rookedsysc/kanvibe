import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/providers.dart';
import '../domain/task_file.dart';

typedef TaskFileTarget = ({String taskId, String path});

final taskFileProvider = FutureProvider.autoDispose
    .family<TaskFile, TaskFileTarget>((ref, target) async {
      final connection = (await ref.watch(connectionControllerProvider.future))
          .activeConnection;
      if (connection == null) throw StateError('연결된 PC가 없습니다.');
      return ref
          .read(desktopClientProvider)
          .fetchTaskFile(connection, target.taskId, target.path);
    }, retry: (_, _) => null);
