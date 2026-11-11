import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_command.dart';
import '../../../common/network/desktop_connection.dart';

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
  } catch (_) {
    return const TaskDeletionFailed('데스크탑에 닿지 못했습니다. 잠시 뒤 다시 시도해 주세요.');
  }
}
