import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../features/board/data/board_snapshot.dart';
import '../features/connection/data/connection_store.dart';
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

/// 저장된 연결. null이면 아직 페어링하지 않은 상태다
class ConnectionController extends AsyncNotifier<DesktopConnection?> {
  @override
  Future<DesktopConnection?> build() =>
      ref.read(connectionStoreProvider).read();

  /// 코드를 확인하고 성공하면 연결을 저장한다. 실패 이유를 문자열로 돌려준다
  Future<String?> connect({
    required String host,
    required int port,
    required String code,
  }) async {
    final token = await ref
        .read(desktopClientProvider)
        .pair(host: host, port: port, code: code, deviceName: 'KanVibe Mobile');

    if (token == null) {
      return '코드가 맞지 않거나 만료되었습니다.';
    }

    final connection = DesktopConnection(host: host, port: port, token: token);
    await ref.read(connectionStoreProvider).write(connection);
    state = AsyncData(connection);
    return null;
  }

  Future<void> disconnect() async {
    await ref.read(connectionStoreProvider).clear();
    state = const AsyncData(null);
  }
}

final connectionControllerProvider =
    AsyncNotifierProvider<ConnectionController, DesktopConnection?>(
      ConnectionController.new,
    );

/// 데스크탑이 기기를 떼어 내면 남은 토큰으로는 어떤 요청도 통과하지 못한다.
///
/// 저장된 연결을 지워야 앱이 페어링 화면으로 되돌아가고 사용자가 다시 붙을 수 있다.
/// 지우는 것은 이 기기에 보관한 값뿐이고 데스크탑에는 아무것도 알리지 않는다.
Future<void> _forgetRejectedConnection(
  Ref ref,
  DesktopRequestException error,
) async {
  if (error.isUnauthorized) {
    await ref.read(connectionControllerProvider.notifier).disconnect();
  }
}

/// 보드 내용. 연결이 바뀌면 자동으로 다시 불러온다
final boardProvider = FutureProvider<BoardSnapshot>((ref) async {
  final connection = await ref.watch(connectionControllerProvider.future);
  if (connection == null) {
    return const BoardSnapshot(tasksByStatus: {});
  }

  try {
    final board = await ref.read(desktopClientProvider).fetchBoard(connection);
    return BoardSnapshot.fromJson(board);
  } on DesktopRequestException catch (error) {
    await _forgetRejectedConnection(ref, error);
    rethrow;
  }
});

/// 태스크 하나의 탭과 pane. 태스크별로 따로 들고 있어야 두 태스크를 오갈 때 섞이지 않는다
final surfacesProvider = FutureProvider.family<TaskSurfaces, String>((
  ref,
  taskId,
) async {
  final connection = await ref.watch(connectionControllerProvider.future);
  if (connection == null) {
    throw const SurfaceUnavailableException(SurfaceUnavailableReason.noSession);
  }

  try {
    return await ref
        .read(desktopClientProvider)
        .fetchSurfaces(connection, taskId);
  } on DesktopRequestException catch (error) {
    await _forgetRejectedConnection(ref, error);
    rethrow;
  }
});
