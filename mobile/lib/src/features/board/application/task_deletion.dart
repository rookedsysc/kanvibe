import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_command.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';

/// 태스크 삭제 요청이 어떻게 끝났는지. 보드와 상태 시트가 같은 결과를 같은 방식으로 안내한다
sealed class TaskDeletionOutcome {
  const TaskDeletionOutcome();
}

/// 지워졌다. 이미 없는 태스크도 지우려던 결과와 같으므로 여기에 든다
class TaskDeletionSucceeded extends TaskDeletionOutcome {
  const TaskDeletionSucceeded({required this.failedPeerCount});

  /// 꺼져 있어 전파받지 못한 다른 데스크탑 수. 그곳에는 태스크가 남아 있다
  final int failedPeerCount;
}

/// 커밋하지 않은 변경이 있어 데스크탑이 멈췄다. 사용자가 강제 삭제를 고르면 다시 보낸다
class TaskDeletionNeedsForce extends TaskDeletionOutcome {
  const TaskDeletionNeedsForce();
}

class TaskDeletionNeedsRegistrationRemoval extends TaskDeletionOutcome {
  const TaskDeletionNeedsRegistrationRemoval();
}

class TaskDeletionFailed extends TaskDeletionOutcome {
  const TaskDeletionFailed(this.message);

  final String message;
}

/// 응답을 기다리다 시간이 넘었다. 데스크탑은 이미 지웠을 수도 있어 단순 실패와 나눠 다시 확인한다
class TaskDeletionUnconfirmed extends TaskDeletionFailed {
  const TaskDeletionUnconfirmed()
    : super('삭제 결과를 확인하지 못했습니다. 잠시 뒤 보드에서 다시 확인해 주세요.');
}

/// 열려 있는 상태 소켓으로 삭제 명령을 보내고 결과를 화면이 안내할 값으로 바꾼다
Future<TaskDeletionOutcome> requestTaskDeletion(
  DesktopClient client,
  DesktopConnection connection,
  String taskId, {
  required bool isForced,
  bool isRegistrationOnly = false,
}) async {
  try {
    final result = await client.runCommand(
      connection,
      isRegistrationOnly
          ? DesktopCommand.removeTaskRegistration(taskId)
          : DesktopCommand.deleteTask(taskId, isForced: isForced),
    );
    final failedPeerCount = result['failedPeerCount'];
    return TaskDeletionSucceeded(
      failedPeerCount: failedPeerCount is int ? failedPeerCount : 0,
    );
  } on DesktopCommandFailure catch (failure) {
    return switch (failure.code) {
      'dirty-worktree' when !isForced && !isRegistrationOnly =>
        const TaskDeletionNeedsForce(),
      'invalid-worktree' when !isRegistrationOnly =>
        const TaskDeletionNeedsRegistrationRemoval(),
      'task-not-found' => const TaskDeletionSucceeded(failedPeerCount: 0),
      _ => TaskDeletionFailed(failure.message),
    };
  } on DesktopUpdateRequiredException {
    return const TaskDeletionFailed(desktopUpdateRequiredMessage);
  } on DesktopDisconnectedException {
    return const TaskDeletionFailed(desktopDisconnectedMessage);
  } on TimeoutException {
    return const TaskDeletionUnconfirmed();
  } catch (_) {
    return const TaskDeletionFailed('데스크탑에 닿지 못했습니다. 잠시 뒤 다시 시도해 주세요.');
  }
}

/// 태스크 하나의 삭제 요청 상태. 업무 상태(Todo·Done 등)와 따로 둔다
sealed class TaskDeletionProgress {
  const TaskDeletionProgress();
}

class TaskDeletionRunning extends TaskDeletionProgress {
  const TaskDeletionRunning();
}

/// 지워졌다. 시트를 먼저 닫았어도 상세 화면이 이 값을 보고 보드로 돌아간다
class TaskDeletionDone extends TaskDeletionProgress {
  const TaskDeletionDone({required this.failedPeerCount});

  final int failedPeerCount;
}

final taskDeletionProvider =
    NotifierProvider<TaskDeletionController, Map<String, TaskDeletionProgress>>(
      TaskDeletionController.new,
    );

/// 삭제 요청을 화면 밖에서 끝까지 들고 있는다.
///
/// 보드 카드와 상태 시트가 같은 진행 상태를 보고, 요청한 위젯이 사라져도 보드 갱신과 결과 반영이 이어진다.
class TaskDeletionController
    extends Notifier<Map<String, TaskDeletionProgress>> {
  @override
  Map<String, TaskDeletionProgress> build() => const {};

  /// 이미 같은 태스크를 지우는 중이면 명령을 다시 보내지 않고 null을 돌려준다
  Future<TaskDeletionOutcome?> delete(
    String taskId, {
    required bool isForced,
    bool isRegistrationOnly = false,
  }) async {
    final connection = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    if (connection == null || state[taskId] is TaskDeletionRunning) {
      return null;
    }

    _setProgress(taskId, const TaskDeletionRunning());
    var outcome = await requestTaskDeletion(
      ref.read(desktopClientProvider),
      connection,
      taskId,
      isForced: isForced,
      isRegistrationOnly: isRegistrationOnly,
    );
    if (outcome is TaskDeletionUnconfirmed) {
      outcome = await _confirmByBoard(taskId);
    }

    if (outcome is TaskDeletionSucceeded) {
      ref.invalidate(boardProvider);
      _setProgress(
        taskId,
        TaskDeletionDone(failedPeerCount: outcome.failedPeerCount),
      );
    } else {
      _setProgress(taskId, null);
    }
    return outcome;
  }

  /// 보드에 태스크가 없으면 데스크탑이 늦게라도 지운 것이다. 남아 있거나 보드도 못 읽으면 결과를 모른다고 알린다
  Future<TaskDeletionOutcome> _confirmByBoard(String taskId) async {
    try {
      final board = await ref.refresh(boardProvider.future);
      final isStillOnBoard = board.tasksByStatus.values.any(
        (tasks) => tasks.any((task) => task.id == taskId),
      );
      return isStillOnBoard
          ? const TaskDeletionUnconfirmed()
          : const TaskDeletionSucceeded(failedPeerCount: 0);
    } catch (_) {
      return const TaskDeletionUnconfirmed();
    }
  }

  void _setProgress(String taskId, TaskDeletionProgress? progress) {
    final next = {...state};
    if (progress == null) {
      next.remove(taskId);
    } else {
      next[taskId] = progress;
    }
    state = next;
  }
}
