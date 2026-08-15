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
  currentTask: string | null;
  lastActiveAt: string | null;
  runningSubtasks: LiveAiSubtask[];
}

/**
 * 세션이 띄운 에이전트 하나.
 * 서브에이전트가 또 서브에이전트를 띄우므로 깊이에 제한이 없고, 그래서 재귀 구조다.
 */
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

/**
 * 세션 하나의 호출 그래프.
 *
 * 목록은 "무엇이 도는가"까지만 답하고, 이게 어떻게 갈라졌는지와 병렬인지 순차인지는 이 그래프가 답한다.
 * 기록 전문을 훑어야 만들 수 있어 폴링에 얹지 않고 상세보기를 열 때만 만든다.
 */
export interface AgentCallGraph {
  provider: AiSessionProvider;
  sessionId: string;
  startedAt: string | null;
  /** 그래프를 읽은 시각. 아직 도는 막대의 오른쪽 끝이 된다 */
  readAt: string;
  roots: AgentCallNode[];
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
