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

/**
 * 세션이 지금 돌고 있는지.
 * `running`은 에이전트 프로세스가 붙어 있거나 세션 기록이 방금 갱신됐다는 뜻이고,
 * `idle`은 세션은 남아 있지만 최근 활동이 없다는 뜻이다.
 */
export type LiveAiSessionState = "running" | "idle";

/** 지금 돌고 있는 서브에이전트 하나 */
export interface LiveAiSubtask {
  id: string;
  name: string | null;
  lastActiveAt: string | null;
}

/** 세션이 붙어 있는 tmux window. 세션을 클릭하면 이 window로 전환한다 */
export interface LiveSessionTerminalWindow {
  sessionName: string;
  windowId: string;
  windowName: string;
}

export interface LiveAiSession {
  provider: AiSessionProvider;
  sessionId: string | null;
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

/**
 * 실행중 판정에 쓰는 두 시간창.
 * `runningWindowMs`는 "방금 움직였다"를, `recentWindowMs`는 "유휴 상태로 보여줄 만큼 최근이다"를 가른다.
 */
export interface LiveAiSessionWindows {
  runningWindowMs: number;
  recentWindowMs: number;
}

/** provider 리더가 돌려주는 실행중 세션 스냅샷 */
export interface LiveProviderSnapshot {
  sessionId: string | null;
  lastActiveAt: string | null;
  runningSubtasks: LiveAiSubtask[];
}

/** tmux pane에서 관측한 실행중 에이전트. 보드 배지는 이 정보만으로 그린다 */
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
