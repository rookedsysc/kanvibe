import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../data/ai_sessions_repository.dart';
import '../domain/live_ai_session.dart';
import '../domain/agent_call_graph.dart';

final aiSessionsRepositoryProvider = Provider<AiSessionsRepository>(
  (ref) => AiSessionsRepository(ref.watch(desktopClientProvider)),
);
final liveAiSessionsProvider = FutureProvider.autoDispose
    .family<LiveAiSessions, String>(
      (ref, taskId) => _query(
        ref,
        (repository, connection) =>
            repository.fetchSessions(connection, taskId),
      ),
      retry: (_, _) => null,
    );
typedef AgentGraphTarget = ({String taskId, String provider, String sessionId});
final agentCallGraphProvider = FutureProvider.autoDispose
    .family<AgentCallGraph?, AgentGraphTarget>(
      (ref, target) => _query(
        ref,
        (repository, connection) => repository.fetchGraph(
          connection,
          target.taskId,
          target.provider,
          target.sessionId,
        ),
      ),
      retry: (_, _) => null,
    );
Future<T> _query<T>(
  Ref ref,
  Future<T> Function(AiSessionsRepository, DesktopConnection) fetch,
) async {
  final connection = (await ref.watch(connectionControllerProvider.future))
      .activeConnection;
  if (connection == null) throw StateError('연결된 PC가 없습니다.');
  try {
    return await fetch(ref.read(aiSessionsRepositoryProvider), connection);
  } on DesktopRequestException catch (error) {
    if (ref.mounted && error.isUnauthorized) {
      final active = ref
          .read(connectionControllerProvider)
          .value
          ?.activeConnection;
      if (active?.id == connection.id && active?.token == connection.token) {
        await ref
            .read(connectionControllerProvider.notifier)
            .forgetLocally(connection.id, connection.token);
      }
    }
    rethrow;
  }
}
