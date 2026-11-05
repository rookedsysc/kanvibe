import { readAdditionalFileSessions, readAdditionalFileSessionDetail } from "@/lib/aiSessions/readAdditionalFileSessions";
import { readSqliteAgentSessions, readSqliteAgentSessionDetail } from "@/lib/aiSessions/readSqliteAgentSessions";
import { isSSHTransportError } from "@/lib/gitOperations";
import { createAggregationResult, createReaderResult, paginateItems, sortSessionsDescending, toSourceStatus } from "@/lib/aiSessions/shared";
import { readClaudeSessionDetail, readClaudeSessions } from "@/lib/aiSessions/readClaudeSessions";
import { readCodexSessionDetail, readCodexSessions } from "@/lib/aiSessions/readCodexSessions";
import { readGeminiSessionDetail, readGeminiSessions } from "@/lib/aiSessions/readGeminiSessions";
import { readOpenCodeSessionDetail, readOpenCodeSessions } from "@/lib/aiSessions/readOpenCodeSessions";
import type { AggregatedAiSessionsResult, AiSessionDetailReaderResult, AiSessionProvider, AiSessionReaderContext, AiSessionReaderResult } from "@/lib/aiSessions/types";

export async function aggregateAiSessions(context: AiSessionReaderContext): Promise<AggregatedAiSessionsResult> {
  const [claude, codex, openCode, gemini, additional, sqlite] = await Promise.all([
    readReaderSafely("claude", context, readClaudeSessions),
    readReaderSafely("codex", context, readCodexSessions),
    readReaderSafely("opencode", context, readOpenCodeSessions),
    readReaderSafely("gemini", context, readGeminiSessions),
    readAdditionalFileSessions(context),
    readSqliteAgentSessions(context),
  ]);

  const readers = [claude, codex, openCode, gemini, ...additional, ...sqlite];
  const allSessions = readers.flatMap((reader) => reader.sessions);
  const sortedSessions = sortSessionsDescending(allSessions);
  const paginatedSessions = context.limit
    ? paginateItems(sortedSessions, context.cursor, context.limit)
    : { items: sortedSessions, nextCursor: null };

  return createAggregationResult({
    isRemote: Boolean(context.sshHost),
    targetPath: context.worktreePath,
    repoPath: context.repoPath,
    sessions: paginatedSessions.items,
    sources: readers.map(toSourceStatus),
    nextCursor: paginatedSessions.nextCursor,
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

    return createReaderResult(provider, { available: false, reason: "Session storage could not be read" });
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
    case "opencode2":
    case "zcode":
      return readSqliteAgentSessionDetail(context, provider, sessionId, sourceRef, cursor, limit);
    default:
      return readAdditionalFileSessionDetail(context, provider, sessionId, sourceRef, cursor, limit);
  }
}
