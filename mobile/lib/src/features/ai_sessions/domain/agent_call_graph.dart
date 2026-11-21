class AgentCallNode {
  const AgentCallNode({
    required this.id,
    required this.children,
    this.agentType,
    this.skill,
    this.task,
    this.startedAt,
    this.endedAt,
  });
  final String id;
  final String? agentType;
  final String? skill;
  final String? task;
  final DateTime? startedAt;
  final DateTime? endedAt;
  final List<AgentCallNode> children;
}

class AgentCallGraph {
  const AgentCallGraph({
    required this.provider,
    required this.sessionId,
    required this.roots,
    required this.readAt,
    this.startedAt,
  });
  final String provider;
  final String sessionId;
  final List<AgentCallNode> roots;
  final DateTime? startedAt;
  final DateTime? readAt;
}
