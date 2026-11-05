

export type AiSessionProvider = "claude" | "codex" | "opencode" | "gemini";

export type AiSessionMatchScope = "worktree";

export type AiMessageRole = "user" | "assistant" | "tool" | "system" | "developer" | "reasoning" | "unknown";

export interface AggregatedAiMessage {
  role: AiMessageRole;
  timestamp: string | null;
  text: string;
  fullText: string;
  isTruncated: boolean;
}

export interface AggregatedAiSession {
  id: string;
  provider: AiSessionProvider;
  startedAt: string | null;
  updatedAt: string | null;
  matchedPath: string | null;
  matchScope: AiSessionMatchScope;
  title: string | null;
  firstUserPrompt: string | null;
  messageCount: number;
  sourceRef?: string | null;
}

export interface AggregatedAiSessionDetail {
  sessionId: string;
  provider: AiSessionProvider;
  title: string | null;
  matchedPath: string | null;
  sourceRef?: string | null;
  messages: AggregatedAiMessage[];
  nextCursor: string | null;
}

export interface AiSessionSourceStatus {
  provider: AiSessionProvider;
  available: boolean;
  sessionCount: number;
  reason: string | null;
}

export interface AggregatedAiSessionsResult {
  isRemote: boolean;
  targetPath: string | null;
  repoPath: string | null;
  sessions: AggregatedAiSession[];
  sources: AiSessionSourceStatus[];
  nextCursor: string | null;
}

export type LiveAiSessionState = "running" | "idle";

export interface LiveAiSubtask {
  id: string;
  name: string | null;
  lastActiveAt: string | null;
}

export interface LiveSessionTerminalWindow {
  sessionName: string;
  windowId: string;
  windowName: string;
}

export interface LiveAiSession {
  provider: AiSessionProvider;
  sessionId: string | null;
  /** 세션이 지금 붙들고 있는 작업. 가장 최근 사용자 요청에서 뽑는다 */
  currentTask: string | null;
  state: LiveAiSessionState;
  lastActiveAt: string | null;
  runningSubtasks: LiveAiSubtask[];
  terminalWindow: LiveSessionTerminalWindow | null;
}

export interface LiveAiSessionsResult {
  taskId: string;
  isRemote: boolean;
  sessions: LiveAiSession[];
}

export interface LiveAiSessionWindows {
  runningWindowMs: number;
  recentWindowMs: number;
}

export interface LiveProviderSnapshot {
  sessionId: string | null;
  currentTask: string | null;
  lastActiveAt: string | null;
  runningSubtasks: LiveAiSubtask[];
}

export interface AgentCallNode {
  id: string;
  /** Explore, general-purpose 같은 에이전트 종류 */
  agentType: string | null;
  /** 그 에이전트가 따르고 있는 skill */
  skill: string | null;
  /** 위임받은 작업 */
  task: string | null;
  startedAt: string | null;
  /** 아직 돌고 있으면 null */
  endedAt: string | null;
  children: AgentCallNode[];
}

export interface AgentCallGraph {
  provider: AiSessionProvider;
  sessionId: string;
  startedAt: string | null;
  /** 그래프를 읽은 시각. 아직 도는 막대의 오른쪽 끝이 된다 */
  readAt: string;
  roots: AgentCallNode[];
}

export interface RunningAgentPane {
  provider: AiSessionProvider;
  worktreePath: string;
  sessionName: string;
  windowId: string;
  windowName: string;
}

export interface AiSessionReaderContext {
  worktreePath: string | null;
  repoPath: string | null;
  query?: string;
  roles?: AiMessageRole[];
  cursor?: string | null;
  limit?: number;
  sshHost?: string | null;
}

export interface AiSessionReaderResult {
  provider: AiSessionProvider;
  available: boolean;
  sessionCount: number;
  reason: string | null;
  sessions: AggregatedAiSession[];
}

export type AiSessionDetailReaderResult = AggregatedAiSessionDetail;
