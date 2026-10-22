import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../features/board/data/board_snapshot.dart';
import '../features/connection/data/connection_store.dart';
import '../features/connection/domain/saved_connections.dart';
import '../features/task_detail/domain/mirror_pane.dart';
import 'network/desktop_client.dart';
import 'network/desktop_connection.dart';

/// 앱 전체가 공유하는 것들.
///
/// 연결 정보와 통신 수단은 화면보다 오래 살아야 해서 여기에 둔다.
/// 화면별 상태(선택한 탭, 열려 있는 pane 구독)는 각 feature가 자기 provider로 들고 있다.

final desktopClientProvider = Provider<DesktopClient>((ref) {
  final client = DesktopClient();
  ref.onDispose(client.close);
  return client;
});

final connectionStoreProvider = Provider<ConnectionStore>(
  (ref) => ConnectionStore(),
);

/// 연결이 성립하지 못한 사유. 화면이 이것을 문구로 옮긴다
enum ConnectFailure { rejectedCode, alreadyPaired }

/// 저장된 모든 PC와 현재 선택한 PC를 함께 관리한다.
class ConnectionController extends AsyncNotifier<SavedConnections> {
  Future<void> _pendingSave = Future.value();

  @override
  Future<SavedConnections> build() =>
      ref.read(connectionStoreProvider).readRegistry();

  /// 코드를 확인하고 성공하면 연결을 저장한다. 성공이면 null, 아니면 실패 사유를 돌려준다.
  ///
  /// 문장이 아니라 사유를 돌려주는 이유는, 컨트롤러가 화면 문구를 들고 있으면
  /// 나중에 번역을 넣을 때 이 계층부터 다시 갈라야 하기 때문이다. 문구는 화면이 고른다.
  Future<ConnectFailure?> connect({
    required String host,
    required int port,
    required String code,
    String name = '',
  }) async {
    final id = DesktopConnection(host: host, port: port, token: '').id;
    final current = await future;
    if (current.find(id) != null) return ConnectFailure.alreadyPaired;

    final token = await ref
        .read(desktopClientProvider)
        .pair(host: host, port: port, code: code, deviceName: 'KanVibe Mobile');

    if (token == null) {
      return ConnectFailure.rejectedCode;
    }

    final connection = DesktopConnection(
      host: host,
      port: port,
      token: token,
      name: name.trim(),
    );
    var added = false;
    await _change((registry) {
      if (registry.find(id) != null) return registry;
      added = true;
      return registry.add(connection);
    });
    return added ? null : ConnectFailure.alreadyPaired;
  }

  /// 현재 PC만 바꾼다. 다른 PC의 페어링 토큰은 그대로 둔다.
  Future<void> select(String id) async {
    await _change(
      (registry) => registry.activeId == id ? registry : registry.select(id),
    );
  }

  /// 지정한 PC를 데스크탑과 이 기기 양쪽에서 제거한다.
  ///
  /// PC가 꺼져 있어도 로컬 목록에서는 제거하고, 원격 해제 실패 여부를 화면에 돌려준다.
  Future<bool> remove(String id) async {
    final current = await future;
    final connection = current.find(id);
    if (connection == null) return true;
    var toldDesktop = true;

    try {
      await ref.read(desktopClientProvider).unpairDevice(connection);
    } on DesktopRequestException catch (error) {
      toldDesktop = error.isUnauthorized;
    } catch (_) {
      toldDesktop = false;
    }

    await forgetLocally(id, connection.token);
    return toldDesktop;
  }

  /// 거절된 토큰의 PC만 지운다. 늦게 끝난 요청이 다른 PC 선택을 지우면 안 된다.
  Future<void> forgetLocally([String? id, String? token]) async {
    await _change((registry) {
      final targetId = id ?? registry.activeId;
      if (targetId == null) return registry;
      final stored = registry.find(targetId);
      return stored == null || (token != null && stored.token != token)
          ? registry
          : registry.remove(targetId);
    });
  }

  /// 저장이 겹쳐도 항상 직전 저장 결과에 변경을 적용한다.
  Future<SavedConnections> _change(
    SavedConnections Function(SavedConnections) transform,
  ) {
    final result = _pendingSave.then((_) async {
      final current = await future;
      final updated = transform(current);
      if (identical(updated, current)) return current;
      await ref.read(connectionStoreProvider).writeRegistry(updated);
      state = AsyncData(updated);
      return updated;
    });
    _pendingSave = result.then<void>(
      (_) {},
      onError: (Object error, StackTrace stack) {},
    );
    return result;
  }
}

final connectionControllerProvider =
    AsyncNotifierProvider<ConnectionController, SavedConnections>(
      ConnectionController.new,
    );

/// 데스크탑이 기기를 떼어 내면 남은 토큰으로는 어떤 요청도 통과하지 못한다.
///
/// 거절된 PC만 지워야 다른 PC의 연결을 계속 쓸 수 있다.
/// 지우는 것은 이 기기에 보관한 값뿐이고 데스크탑에는 아무것도 알리지 않는다.
/// 이미 거절당한 토큰을 다시 실어 보내 봐야 401만 한 번 더 받을 뿐이다.
Future<void> _forgetRejectedConnection(
  Ref ref,
  DesktopRequestException error,
  DesktopConnection connection,
) async {
  if (error.isUnauthorized) {
    await ref
        .read(connectionControllerProvider.notifier)
        .forgetLocally(connection.id, connection.token);
  }
}

/// 실패를 되불러 가지 않는다.
///
/// Riverpod 3은 provider가 던지면 기본으로 열 번까지 되불러 가고, 그동안 상태를 `AsyncLoading`으로 둔다.
/// 여기서 던지는 것은 "세션이 없다", "데스크탑에 닿지 못한다"처럼 기다린다고 풀리지 않는 것들이라,
/// 화면이 곧바로 안내해야 할 자리에 40초 가까이 회전만 돌게 된다. 첫 실패를 그대로 화면에 넘기고
/// 다시 시도는 사용자가 누르게 한다.
Duration? _neverRetry(int retryCount, Object error) => null;

/// 보드 내용. 연결이 바뀌면 자동으로 다시 불러온다
final boardProvider = FutureProvider<BoardSnapshot>((ref) async {
  final connection = (await ref.watch(connectionControllerProvider.future))
      .activeConnection;
  if (connection == null) {
    return const BoardSnapshot(tasksByStatus: {});
  }

  try {
    final board = await ref.read(desktopClientProvider).fetchBoard(connection);
    return BoardSnapshot.fromJson(board);
  } on DesktopRequestException catch (error) {
    await _forgetRejectedConnection(ref, error, connection);
    rethrow;
  }
}, retry: _neverRetry);

/// 태스크 하나의 탭과 pane. 태스크별로 따로 들고 있어야 두 태스크를 오갈 때 섞이지 않는다
final surfacesProvider = FutureProvider.family<TaskSurfaces, String>((
  ref,
  taskId,
) async {
  final connection = (await ref.watch(connectionControllerProvider.future))
      .activeConnection;
  if (connection == null) {
    throw const SurfaceUnavailableException(SurfaceUnavailableReason.noSession);
  }

  try {
    return await ref
        .read(desktopClientProvider)
        .fetchSurfaces(connection, taskId);
  } on DesktopRequestException catch (error) {
    await _forgetRejectedConnection(ref, error, connection);
    rethrow;
  }
}, retry: _neverRetry);

/// 프로젝트 하나의 브랜치 목록. 태스크 생성 화면의 베이스 브랜치 칸이 쓴다
final projectBranchesProvider = FutureProvider.family<List<String>, String>((
  ref,
  projectId,
) async {
  final connection = (await ref.watch(connectionControllerProvider.future))
      .activeConnection;
  if (connection == null) {
    return const [];
  }

  try {
    return await ref
        .read(desktopClientProvider)
        .fetchProjectBranches(connection, projectId);
  } on DesktopRequestException catch (error) {
    await _forgetRejectedConnection(ref, error, connection);
    rethrow;
  }
}, retry: _neverRetry);
