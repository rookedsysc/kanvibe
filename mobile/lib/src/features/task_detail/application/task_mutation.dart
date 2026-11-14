import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../../board/domain/task_status.dart';
import '../../task_create/domain/session_type.dart';

/// 태스크 하나의 상태 이동·세션 변경 요청 상태. 업무 상태(Todo·Done 등)와 따로 둔다
sealed class TaskMutationProgress {
  const TaskMutationProgress();
}

class TaskStatusMoving extends TaskMutationProgress {
  const TaskStatusMoving(this.status);

  final TaskStatus status;
}

/// 옮겼다. 시트를 먼저 닫았어도 상세 화면이 이 값을 보고 태스크 상태를 바꾼다
class TaskStatusMoved extends TaskMutationProgress {
  const TaskStatusMoved(this.status);

  final TaskStatus status;
}

class TaskSessionTypeChanging extends TaskMutationProgress {
  const TaskSessionTypeChanging(this.sessionType);

  final SessionType sessionType;
}

/// 요청이 어떻게 끝났는지. 시트는 실패 사유만 남기고 성공하면 닫힌다
sealed class TaskMutationOutcome {
  const TaskMutationOutcome();
}

class TaskMutationSucceeded extends TaskMutationOutcome {
  const TaskMutationSucceeded();
}

class TaskMutationFailed extends TaskMutationOutcome {
  const TaskMutationFailed(this.message);

  final String message;
}

/// 연결이 없거나 같은 태스크의 다른 요청이 진행 중이라 보내지 않았다
class TaskMutationSkipped extends TaskMutationOutcome {
  const TaskMutationSkipped();
}

final taskMutationProvider =
    NotifierProvider<TaskMutationController, Map<String, TaskMutationProgress>>(
      TaskMutationController.new,
    );

/// 상태 이동과 세션 변경을 화면 밖에서 끝까지 들고 있는다.
///
/// 요청한 시트가 먼저 닫혀도 보드 갱신과 상세 화면 반영이 이어진다. 한 태스크에는 한 번에 한 요청만 보낸다.
class TaskMutationController
    extends Notifier<Map<String, TaskMutationProgress>> {
  @override
  Map<String, TaskMutationProgress> build() => const {};

  bool isBusy(String taskId) {
    final progress = state[taskId];
    return progress is TaskStatusMoving || progress is TaskSessionTypeChanging;
  }

  Future<TaskMutationOutcome> moveStatus(
    String taskId,
    TaskStatus status,
  ) async {
    final connection = _activeConnection();
    if (connection == null || isBusy(taskId)) {
      return const TaskMutationSkipped();
    }

    _setProgress(taskId, TaskStatusMoving(status));
    try {
      await ref
          .read(desktopClientProvider)
          .updateTaskStatus(connection, taskId, status);
    } catch (error) {
      _setProgress(taskId, null);
      return TaskMutationFailed(_moveFailureMessage(error));
    }

    // 보드에도 같은 변화가 반영돼야 돌아갔을 때 옛 칸에 남아 있지 않다
    ref.invalidate(boardProvider);
    _setProgress(taskId, TaskStatusMoved(status));
    return const TaskMutationSucceeded();
  }

  /// 기존 세션을 닫을지는 시트가 먼저 묻고 그 답을 넘긴다
  Future<TaskMutationOutcome> changeSessionType(
    String taskId,
    SessionType sessionType, {
    required bool shouldCloseExistingSession,
  }) async {
    final connection = _activeConnection();
    if (connection == null || isBusy(taskId)) {
      return const TaskMutationSkipped();
    }

    _setProgress(taskId, TaskSessionTypeChanging(sessionType));
    try {
      await ref
          .read(desktopClientProvider)
          .changeTaskSessionType(
            connection,
            taskId,
            sessionType: sessionType,
            shouldCloseExistingSession: shouldCloseExistingSession,
          );
    } catch (error) {
      _setProgress(taskId, null);
      return TaskMutationFailed(sessionTypeChangeFailureMessage(error));
    }

    // 데스크탑이 이전 스트림을 끊기 전이라도 새 세션의 pane을 바로 받아 화면을 맞춘다
    ref.invalidate(surfacesProvider(taskId));
    _setProgress(taskId, null);
    return const TaskMutationSucceeded();
  }

  DesktopConnection? _activeConnection() =>
      ref.read(connectionControllerProvider).value?.activeConnection;

  void _setProgress(String taskId, TaskMutationProgress? progress) {
    final next = {...state};
    if (progress == null) {
      next.remove(taskId);
    } else {
      next[taskId] = progress;
    }
    state = next;
  }

  String _moveFailureMessage(Object error) {
    if (error is! DesktopRequestException) {
      return '데스크탑에 닿지 못했습니다. 잠시 뒤 다시 시도해 주세요.';
    }
    if (error.statusCode == 404) {
      return '데스크탑에서 이미 지워진 태스크입니다.';
    }
    return error.message ?? '상태를 옮기지 못했습니다.';
  }
}

/// 세션 종류 변경 실패를 안내할 문장. 시트의 사전 확인 실패도 같은 문장으로 알린다
String sessionTypeChangeFailureMessage(Object error) {
  if (error is! DesktopRequestException) {
    return '데스크탑에 닿지 못했습니다. 잠시 뒤 다시 시도해 주세요.';
  }
  if (error.statusCode == 404) {
    return '데스크탑에서 이미 지워진 태스크입니다.';
  }
  return error.message ?? '세션 종류를 바꾸지 못했습니다.';
}
