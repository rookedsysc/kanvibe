import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/providers.dart';
import '../../board/data/board_snapshot.dart';
import '../../board/domain/board_project.dart';
import '../../board/domain/board_task.dart';

/// 생성 응답을 기다리다 시간이 넘었을 때 안내할 문장. 데스크탑이 이미 만들었을 수도 있어 실패라고 단정하지 않는다
const unconfirmedTaskCreationMessage = '생성 결과를 확인하지 못했습니다. 잠시 뒤 보드에서 확인해 주세요.';

/// 다시 읽은 보드에서 방금 만들려던 태스크를 찾는다. 같은 프로젝트에 같은 브랜치가 있으면 데스크탑이 늦게라도 만든 것이다
BoardTask? findCreatedTaskOnBoard(
  BoardSnapshot board, {
  required String projectId,
  required String branchName,
}) {
  for (final tasks in board.tasksByStatus.values) {
    for (final task in tasks) {
      if (task.projectId == projectId && task.branchName == branchName) {
        return task;
      }
    }
  }
  return null;
}

/// 태스크 생성 요청이 어떻게 끝났는지
sealed class TaskCreationOutcome {
  const TaskCreationOutcome();
}

class TaskCreationSucceeded extends TaskCreationOutcome {
  const TaskCreationSucceeded(this.task);

  final BoardTask task;
}

class TaskCreationFailed extends TaskCreationOutcome {
  const TaskCreationFailed(this.message);

  final String message;
}

/// 응답이 늦었고 다시 읽은 보드에서도 찾지 못했다. 데스크탑이 아직 만드는 중일 수도 있어 실패와 따로 안내한다
class TaskCreationUnconfirmed extends TaskCreationFailed {
  const TaskCreationUnconfirmed() : super(unconfirmedTaskCreationMessage);
}

/// 연결이 없어 보내지 않았다
class TaskCreationSkipped extends TaskCreationOutcome {
  const TaskCreationSkipped();
}

final taskCreationServiceProvider = Provider<TaskCreationService>(
  TaskCreationService.new,
);

/// 생성 요청을 화면 밖에서 끝까지 들고 있는다.
///
/// 생성은 worktree와 세션을 만드느라 오래 걸리므로, 사용자가 화면을 나가도 요청을 이어 가고
/// 보드 갱신과 결과 판정을 마친다. 결과를 어떻게 알릴지는 요청한 화면이 정한다.
class TaskCreationService {
  TaskCreationService(this._ref);

  final Ref _ref;

  /// 응답이 늦어지면 [onConfirming]을 부르고 보드를 다시 읽어 결과를 정한다
  Future<TaskCreationOutcome> create({
    required BoardProject project,
    required String branchName,
    String? baseBranch,
    required String description,
    String? priority,
    String? sessionType,
    String? existingTaskId,
    void Function()? onConfirming,
  }) async {
    final connection = _ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    if (connection == null) {
      return const TaskCreationSkipped();
    }

    final Map<String, dynamic> created;
    try {
      if (existingTaskId != null) {
        // 기존 태스크를 브랜치 작업으로 바꾼다. 같은 브랜치가 생기므로 늦은 응답도 보드에서 같은 방식으로 확인한다
        final response = await _ref
            .read(desktopClientProvider)
            .requestJson(
              connection,
              'POST',
              ['tasks', existingTaskId, 'branch'],
              body: {
                'projectId': project.id,
                'branchName': branchName,
                'baseBranch': baseBranch ?? project.defaultBranch,
                'sessionType': sessionType,
              },
              timeout: Duration(seconds: 60),
            );
        created = response['task'] as Map<String, dynamic>;
      } else {
        created = await _ref
            .read(desktopClientProvider)
            .createTask(
              connection,
              projectId: project.id,
              branchName: branchName,
              baseBranch: baseBranch,
              description: description,
              priority: priority,
              sessionType: sessionType,
            );
      }
    } on TimeoutException {
      onConfirming?.call();
      return _confirmByBoard(projectId: project.id, branchName: branchName);
    } catch (error) {
      return TaskCreationFailed(_createFailureMessage(error));
    }

    final task = BoardTask.fromJson(
      created,
      {project.id: project.name},
      projectColorsById: {
        if (project.color != null) project.id: project.color!,
      },
    );
    // 데스크탑은 만들었다고 했지만 응답을 읽지 못하면 보드에서 찾아 결과를 정한다
    if (task == null) {
      return _confirmByBoard(projectId: project.id, branchName: branchName);
    }

    // 보드에도 새 태스크가 보여야 돌아갔을 때 빈자리가 아니다
    _ref.invalidate(boardProvider);
    return TaskCreationSucceeded(task);
  }

  /// 응답이 늦었다고 생성이 취소된 것은 아니다. 보드를 다시 읽어 태스크가 있으면 만든 것으로 본다
  Future<TaskCreationOutcome> _confirmByBoard({
    required String projectId,
    required String branchName,
  }) async {
    try {
      final board = await _ref.refresh(boardProvider.future);
      final created = findCreatedTaskOnBoard(
        board,
        projectId: projectId,
        branchName: branchName,
      );
      if (created != null) {
        return TaskCreationSucceeded(created);
      }
    } catch (_) {
      // 보드도 못 읽으면 결과를 모른다고 알린다
    }
    return const TaskCreationUnconfirmed();
  }

  /// 데스크탑이 값을 거절한 것만 사유를 그대로 옮긴다.
  ///
  /// 그 밖의 실패 메시지에는 git 명령줄과 데스크탑의 경로가 섞여 있어 기기로 오지 않는다.
  /// 무엇이 잘못됐는지는 데스크탑에서만 볼 수 있으므로 그쪽을 보라고 말해 준다.
  String _createFailureMessage(Object error) {
    if (error is! DesktopRequestException) {
      return '데스크탑에 닿지 못했습니다. 잠시 뒤 다시 시도해 주세요.';
    }
    return error.message ?? '태스크를 만들지 못했습니다. 데스크탑에서 자세한 사유를 확인해 주세요.';
  }
}
