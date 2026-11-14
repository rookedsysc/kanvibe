import 'dart:async';
import 'dart:convert';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';
import 'package:kanvibe_mobile/src/features/notifications/application/sync_review_controller.dart';
import 'package:kanvibe_mobile/src/features/notifications/application/sync_review_cleanup_controller.dart';
import 'package:kanvibe_mobile/src/features/notifications/data/sync_review_repository.dart';

import '../ai_accounts/ai_test_support.dart';

Map<String, dynamic> reviewPayload() => {
  'success': true,
  'review': {
    'mergedPullRequests': [
      for (final id in ['one', 'two'])
        {
          'taskId': id,
          'taskTitle': 'Merged $id',
          'branchName': 'feat/$id',
          'prUrl': 'https://example.test/pr/$id',
          'mergedAt': '2026-11-13T00:00:00Z',
        },
    ],
    'registeredWorktrees': [
      {
        'taskId': 'new',
        'projectName': 'Project',
        'branchName': 'feat/new',
        'worktreePath': '/fixture/new',
        'sshHost': 'work',
      },
    ],
    'pulledTasks': [
      {
        'taskId': 'pull',
        'taskTitle': 'Pulled',
        'branchName': 'feat/pull',
        'status': 'updated',
        'summary': 'Fast-forward',
      },
    ],
    'failures': [
      {'operation': 'task-pull-sync', 'target': 'failure', 'reason': 'dirty'},
    ],
  },
};
ProviderContainer reviewContainer(
  AiStreamClient client, {
  bool keepListener = true,
}) {
  final container = ProviderContainer(
    overrides: [
      desktopClientProvider.overrideWithValue(client),
      connectionStoreProvider.overrideWithValue(AiMemoryConnections()),
    ],
  );
  if (keepListener) container.listen(syncReviewProvider('notice'), (_, _) {});
  addTearDown(() {
    container.dispose();
    client.close();
  });
  return container;
}

void main() {
  test('정리를 시작하지 않은 검토는 창을 닫으면 보존하지 않는다', () async {
    final client = AiStreamClient((_) async => aiResponse(reviewPayload()));
    final container = reviewContainer(client, keepListener: false);
    final provider = syncReviewProvider('notice');
    final viewing = container.listen(provider, (_, _) {});
    await container.read(provider.future);
    viewing.close();
    await container.pump();
    expect(container.exists(provider), isFalse);
  });

  test('다른 검토창도 같은 정리 작업을 소유하고 중복 정리를 막는다', () async {
    final pending = Completer<http.Response>();
    var requests = 0;
    final client = AiStreamClient((request) async {
      if (request.method != 'POST') return aiResponse(reviewPayload());
      if (++requests == 1) return pending.future;
      return aiResponse({
        'success': true,
        'deletedTaskIds': ['two'],
      });
    });
    final container = reviewContainer(client);
    final first = syncReviewProvider('notice');
    final second = syncReviewProvider('another');
    final viewing = container.listen(second, (_, _) {});
    await container.read(first.future);
    await container.read(second.future);
    container.read(first.notifier).toggle('one');
    container.read(second.notifier).toggle('two');
    final deleting = container.read(first.notifier).deleteSelected();
    expect(container.read(syncReviewCleanupProvider)?.notificationId, 'notice');
    expect(await container.read(second.notifier).deleteSelected(), isFalse);
    pending.complete(
      aiResponse({
        'success': true,
        'deletedTaskIds': ['one'],
      }),
    );
    expect(await deleting, isTrue);
    expect(container.read(syncReviewCleanupProvider), isNull);
    expect(await container.read(second.notifier).deleteSelected(), isTrue);
    viewing.close();
  });

  test('삭제 대기 중 container가 폐기되면 늦은 결과를 쓰지 않는다', () async {
    final pending = Completer<http.Response>();
    final client = AiStreamClient(
      (request) async => request.method == 'POST'
          ? pending.future
          : aiResponse(reviewPayload()),
    );
    final container = reviewContainer(client);
    final provider = syncReviewProvider('notice');
    await container.read(provider.future);
    container.read(provider.notifier).toggle('one');
    final deleting = container.read(provider.notifier).deleteSelected();
    container.dispose();
    pending.complete(
      aiResponse({
        'success': true,
        'deletedTaskIds': ['one'],
      }),
    );
    expect(await deleting, isFalse);
  });

  test('같은 PC의 token이 바뀌면 이전 정리와 결과를 새 연결에서 제외한다', () async {
    final pending = Completer<http.Response>();
    final client = AiStreamClient(
      (request) async => request.method == 'POST'
          ? pending.future
          : aiResponse(reviewPayload()),
    );
    final store = AiMemoryConnections();
    final container = ProviderContainer(
      overrides: [
        desktopClientProvider.overrideWithValue(client),
        connectionStoreProvider.overrideWithValue(store),
      ],
    );
    final provider = syncReviewProvider('notice');
    final viewing = container.listen(provider, (_, _) {});
    addTearDown(() {
      container.dispose();
      client.close();
    });
    await container.read(provider.future);
    container.read(provider.notifier).toggle('one');
    final deleting = container.read(provider.notifier).deleteSelected();
    store.registry = SavedConnections(
      connections: [
        DesktopConnection(
          host: fixturePc.host,
          port: fixturePc.port,
          token: 'replacement',
        ),
      ],
      activeId: fixturePc.id,
    );
    container.invalidate(connectionControllerProvider);
    await container.read(connectionControllerProvider.future);
    await container.read(provider.future);
    expect(container.read(syncReviewCleanupProvider), isNull);
    pending.complete(
      aiResponse({
        'success': true,
        'deletedTaskIds': ['one'],
      }),
    );
    expect(await deleting, isFalse);
    expect(container.read(provider).requireValue.deletedTaskIds, isEmpty);
    viewing.close();
  });

  test('검토창을 닫아도 삭제가 계속되고 재열기 때 같은 결과를 보존한다', () async {
    final pending = Completer<http.Response>();
    final client = AiStreamClient(
      (request) async => request.method == 'POST'
          ? pending.future
          : aiResponse(reviewPayload()),
    );
    final container = reviewContainer(client, keepListener: false);
    final provider = syncReviewProvider('notice');
    final viewing = container.listen(provider, (_, _) {});
    await container.read(provider.future);
    container.read(provider.notifier).toggle('one');
    final deleting = container.read(provider.notifier).deleteSelected();
    viewing.close();
    await container.pump();
    expect(container.exists(provider), isTrue);
    expect(container.read(provider).requireValue.isDeleting, isTrue);
    pending.complete(
      aiResponse({
        'success': true,
        'deletedTaskIds': ['one'],
      }),
    );
    expect(await deleting, isTrue);
    await container.pump();
    final reopened = container.listen(provider, (_, _) {});
    expect((await container.read(provider.future)).deletedTaskIds, {'one'});
    reopened.close();
  });

  test('부분 실패는 성공한 태스크를 남기고 실패한 선택만 다시 삭제한다', () async {
    final sent = <String>[];
    var fail = true;
    final client = AiStreamClient((request) async {
      if (request.method != 'POST') return aiResponse(reviewPayload());
      final ids = List<String>.from(
        (jsonDecode(request.body) as Map)['taskIds'] as List,
      );
      sent.addAll(ids);
      if (ids.contains('two') && fail) {
        return aiResponse({
          'success': false,
          'error': 'dirty worktree',
        }, status: 409);
      }
      return aiResponse({'success': true, 'deletedTaskIds': ids});
    });
    final container = reviewContainer(client);
    final provider = syncReviewProvider('notice');
    await container.read(provider.future);
    final controller = container.read(provider.notifier);
    controller.toggle('one');
    controller.toggle('two');
    expect(await controller.deleteSelected(), isFalse);
    expect(container.read(provider).requireValue.deletedTaskIds, {'one'});
    expect(container.read(provider).requireValue.selectedTaskIds, {'two'});
    fail = false;
    expect(await controller.deleteSelected(), isTrue);
    expect(sent, ['one', 'two', 'two']);
  });

  test('검토의 네 결과 종류와 선택한 task ID만 전송한다', () async {
    final client = AiStreamClient((request) async {
      if (request.method == 'POST') {
        expect(jsonDecode(request.body), {
          'taskIds': ['two'],
        });
        return aiResponse({
          'success': true,
          'deletedTaskIds': ['two'],
        });
      }
      return aiResponse(reviewPayload());
    });
    final repository = SyncReviewRepository(client);
    final result = await repository.fetch(fixturePc, 'notice');
    expect(result.mergedPullRequests.length, 2);
    expect(result.registeredWorktrees.single.sshHost, 'work');
    expect(result.pulledTasks.single.summary, 'Fast-forward');
    expect(result.failures.single.reason, 'dirty');
    expect(await repository.deleteTasks(fixturePc, 'notice', {'two'}), ['two']);
    client.close();
  });
  test('알 수 없는 선택과 빈 삭제를 막고 성공한 삭제는 재조회 후에도 선택할 수 없다', () async {
    var deletes = 0;
    final client = AiStreamClient((request) async {
      if (request.method == 'POST') {
        deletes++;
        return aiResponse({
          'success': true,
          'deletedTaskIds': ['one'],
        });
      }
      return aiResponse(reviewPayload());
    });
    final container = reviewContainer(client);
    final provider = syncReviewProvider('notice');
    await container.read(provider.future);
    final controller = container.read(provider.notifier);
    controller.toggle('outside');
    expect(await controller.deleteSelected(), isFalse);
    controller.toggle('one');
    expect(await controller.deleteSelected(), isTrue);
    expect(deletes, 1);
    expect(container.read(provider).requireValue.deletedTaskIds, {'one'});
    container.invalidate(provider);
    await container.read(provider.future);
    container.read(provider.notifier).toggle('one');
    expect(container.read(provider).requireValue.selectedTaskIds, isEmpty);
  });
  test('삭제 실패는 선택을 유지해 재시도할 수 있다', () async {
    var fail = true;
    final client = AiStreamClient(
      (request) async => request.method == 'POST'
          ? fail
                ? aiResponse({
                    'success': false,
                    'error': 'dirty worktree',
                  }, status: 409)
                : aiResponse({
                    'success': true,
                    'deletedTaskIds': ['two'],
                  })
          : aiResponse(reviewPayload()),
    );
    final container = reviewContainer(client);
    final provider = syncReviewProvider('notice');
    await container.read(provider.future);
    final controller = container.read(provider.notifier);
    controller.toggle('two');
    expect(await controller.deleteSelected(), isFalse);
    expect(container.read(provider).requireValue.selectedTaskIds, {'two'});
    expect(container.read(provider).requireValue.error, isNotNull);
    fail = false;
    expect(await controller.deleteSelected(), isTrue);
  });
  test('PC 변경 후 도착한 삭제 결과는 폐기한다', () async {
    final pending = Completer<http.Response>();
    final client = AiStreamClient(
      (request) async => request.method == 'POST'
          ? pending.future
          : aiResponse(reviewPayload()),
    );
    final container = reviewContainer(client);
    final provider = syncReviewProvider('notice');
    await container.read(provider.future);
    container.read(provider.notifier).toggle('one');
    final deleting = container.read(provider.notifier).deleteSelected();
    await container
        .read(connectionControllerProvider.notifier)
        .select(otherPc.id);
    await container.read(provider.future);
    pending.complete(
      aiResponse({
        'success': true,
        'deletedTaskIds': ['one'],
      }),
    );
    expect(await deleting, isFalse);
    expect(container.read(provider).requireValue.deletedTaskIds, isEmpty);
  });
}
