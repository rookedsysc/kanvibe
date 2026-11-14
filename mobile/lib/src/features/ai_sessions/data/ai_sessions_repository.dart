import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../domain/live_ai_session.dart';
import '../domain/agent_call_graph.dart';

class AiSessionsRepository {
  const AiSessionsRepository(this.client);
  final DesktopClient client;
  Future<LiveAiSessions> fetchSessions(
    DesktopConnection connection,
    String taskId,
  ) async {
    final envelope = await client.requestJson(connection, 'GET', [
      'tasks',
      taskId,
      'live-ai-sessions',
    ]);
    final value = Map<String, dynamic>.from(envelope['liveSessions'] as Map);
    return LiveAiSessions(
      taskId: value['taskId'] as String,
      isRemote: value['isRemote'] == true,
      sessions: List.unmodifiable(
        (value['sessions'] as List).map((raw) {
          final session = Map<String, dynamic>.from(raw as Map);
          final window = session['terminalWindow'] as Map?;
          return LiveAiSession(
            provider: session['provider'] as String,
            state: session['state'] as String,
            sessionId: session['sessionId'] as String?,
            currentTask: session['currentTask'] as String?,
            lastActiveAt: _date(session['lastActiveAt']),
            terminalWindowId: window?['windowId'] as String?,
            terminalWindowName: window?['windowName'] as String?,
            runningSubtasks: List.unmodifiable(
              (session['runningSubtasks'] as List? ?? []).map((rawSubtask) {
                final subtask = rawSubtask as Map;
                return LiveAiSubtask(
                  id: subtask['id'] as String,
                  name: subtask['name'] as String?,
                  lastActiveAt: _date(subtask['lastActiveAt']),
                );
              }),
            ),
          );
        }),
      ),
    );
  }

  Future<AgentCallGraph?> fetchGraph(
    DesktopConnection connection,
    String taskId,
    String provider,
    String sessionId,
  ) async {
    final envelope = await client.requestJson(
      connection,
      'GET',
      ['tasks', taskId, 'agent-call-graph'],
      query: {'provider': provider, 'sessionId': sessionId},
    );
    if (envelope['graph'] == null) return null;
    final graph = envelope['graph'] as Map;
    return AgentCallGraph(
      provider: graph['provider'] as String,
      sessionId: graph['sessionId'] as String,
      startedAt: _date(graph['startedAt']),
      readAt: _date(graph['readAt']),
      roots: List.unmodifiable((graph['roots'] as List).map(_node)),
    );
  }
}

DateTime? _date(Object? value) => DateTime.tryParse(value?.toString() ?? '');
AgentCallNode _node(Object? raw) {
  final value = raw as Map;
  return AgentCallNode(
    id: value['id'] as String,
    agentType: value['agentType'] as String?,
    skill: value['skill'] as String?,
    task: value['task'] as String?,
    startedAt: _date(value['startedAt']),
    endedAt: _date(value['endedAt']),
    children: List.unmodifiable((value['children'] as List? ?? []).map(_node)),
  );
}
