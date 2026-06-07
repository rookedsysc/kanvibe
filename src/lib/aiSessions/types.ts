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
