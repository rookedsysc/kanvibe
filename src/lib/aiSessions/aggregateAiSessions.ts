import { isSSHTransportError } from "@/lib/gitOperations";
import { createAggregationResult, createReaderResult, sortSessionsDescending, toSourceStatus } from "@/lib/aiSessions/shared";
import { readClaudeSessionDetail, readClaudeSessions } from "@/lib/aiSessions/readClaudeSessions";
import { readCodexSessionDetail, readCodexSessions } from "@/lib/aiSessions/readCodexSessions";
import { readGeminiSessionDetail, readGeminiSessions } from "@/lib/aiSessions/readGeminiSessions";
import { readOpenCodeSessionDetail, readOpenCodeSessions } from "@/lib/aiSessions/readOpenCodeSessions";
import type { AggregatedAiSessionsResult, AiSessionDetailReaderResult, AiSessionProvider, AiSessionReaderContext, AiSessionReaderResult } from "@/lib/aiSessions/types";

export async function aggregateAiSessions(context: AiSessionReaderContext): Promise<AggregatedAiSessionsResult> {
  const [claude, codex, openCode, gemini] = await Promise.all([
    readReaderSafely("claude", context, readClaudeSessions),
    readReaderSafely("codex", context, readCodexSessions),
    readReaderSafely("opencode", context, readOpenCodeSessions),
    readReaderSafely("gemini", context, readGeminiSessions),
  ]);

  const allSessions = [...claude.sessions, ...codex.sessions, ...openCode.sessions, ...gemini.sessions];

  return createAggregationResult({
    isRemote: Boolean(context.sshHost),
    targetPath: context.worktreePath,
    repoPath: context.repoPath,
    sessions: sortSessionsDescending(allSessions),
    sources: [claude, codex, openCode, gemini].map(toSourceStatus),
  });
}

async function readReaderSafely(
  provider: AiSessionProvider,
  context: AiSessionReaderContext,
  reader: (context: AiSessionReaderContext) => Promise<AiSessionReaderResult>,
): Promise<AiSessionReaderResult> {
  try {
    return await reader(context);
  } catch (error) {
    if (context.sshHost && isSSHTransportError(error)) {
      return createReaderResult(provider, {
        available: false,
        sessionCount: 0,
        sessions: [],
        reason: `SSH connection to ${context.sshHost} is unavailable`,
      });
    }

    throw error;
  }
}

export async function getAiSessionDetail(
  context: AiSessionReaderContext,
  provider: AiSessionProvider,
  sessionId: string,
  sourceRef?: string | null,
  cursor?: string | null,
  limit?: number
): Promise<AiSessionDetailReaderResult | null> {
  switch (provider) {
    case "claude":
      return readClaudeSessionDetail(context, sessionId, sourceRef, cursor, limit);
    case "codex":
      return readCodexSessionDetail(context, sessionId, sourceRef, cursor, limit);
    case "opencode":
      return readOpenCodeSessionDetail(context, sessionId, sourceRef, cursor, limit);
    case "gemini":
      return readGeminiSessionDetail(context, sessionId, sourceRef, cursor, limit);
    default:
      return null;
  }
}
