import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../../board/domain/board_task.dart';
import '../domain/task_notification.dart';

final notificationNavigationControllerProvider = Provider(
  NotificationNavigationController.new,
);

class NotificationTaskDestination {
  const NotificationTaskDestination({
    required this.connection,
    required this.task,
  });

  final DesktopConnection connection;
  final BoardTask task;
}

class NotificationNavigationException implements Exception {
  const NotificationNavigationException(this.message);

  final String message;
}

/// 저장한 연결만 사용하고, 목록 페이지 밖의 태스크도 서버에서 직접 찾는다.
class NotificationNavigationController {
  NotificationNavigationController(this.ref);

  final Ref ref;
  int _generation = 0;

  void cancel() => _generation += 1;

  bool _isCurrent(int generation) => ref.mounted && generation == _generation;

  Future<NotificationTaskDestination?> prepareDestination(
    TaskNotification notification,
  ) async {
    final generation = ++_generation;
    DesktopConnection? connection;
    try {
      final registry = await ref.read(connectionControllerProvider.future);
      if (!_isCurrent(generation)) return null;
      connection = registry.find(notification.connectionId);
      if (connection == null) {
        throw const NotificationNavigationException('이 알림의 PC 연결이 해제되었습니다.');
      }
      final json = await ref
          .read(desktopClientProvider)
          .fetchTask(connection, notification.taskId);
      if (!_isCurrent(generation)) return null;
      final current = await ref.read(connectionControllerProvider.future);
      if (!_isCurrent(generation) ||
          current.activeId != registry.activeId ||
          current.find(connection.id)?.token != connection.token) {
        return null;
      }
      final task = json == null ? null : BoardTask.fromJson(json, const {});
      if (task == null || task.id != notification.taskId) {
        throw const NotificationNavigationException(
          '태스크가 삭제되었거나 더 이상 존재하지 않습니다.',
        );
      }
      await ref
          .read(connectionControllerProvider.notifier)
          .select(connection.id);
      if (!_isCurrent(generation)) return null;
      final selected = await ref.read(connectionControllerProvider.future);
      if (!_isCurrent(generation) ||
          selected.activeId != connection.id ||
          selected.find(connection.id)?.token != connection.token) {
        return null;
      }
      ref.invalidate(boardProvider);
      ref.invalidate(surfacesProvider(task.id));
      return NotificationTaskDestination(connection: connection, task: task);
    } on NotificationNavigationException {
      rethrow;
    } on DesktopRequestException catch (error) {
      if (!_isCurrent(generation)) return null;
      if (error.isUnauthorized && connection != null) {
        await ref
            .read(connectionControllerProvider.notifier)
            .forgetLocally(connection.id, connection.token);
        throw const NotificationNavigationException('이 알림의 PC 연결이 해제되었습니다.');
      }
      throw const NotificationNavigationException(
        'PC에 연결할 수 없습니다. 연결을 확인한 뒤 다시 시도해주세요.',
      );
    } catch (_) {
      if (!_isCurrent(generation)) return null;
      throw const NotificationNavigationException(
        '태스크를 열지 못했습니다. 잠시 후 다시 시도해주세요.',
      );
    }
  }
}
