import { createAggregationResult, sortSessionsDescending, toSourceStatus } from "@/lib/aiSessions/shared";
import { readClaudeSessionDetail, readClaudeSessions } from "@/lib/aiSessions/readClaudeSessions";
import { readCodexSessionDetail, readCodexSessions } from "@/lib/aiSessions/readCodexSessions";
import { readGeminiSessionDetail, readGeminiSessions } from "@/lib/aiSessions/readGeminiSessions";
import { readOpenCodeSessionDetail, readOpenCodeSessions } from "@/lib/aiSessions/readOpenCodeSessions";
import type { AggregatedAiSessionsResult, AiSessionDetailReaderResult, AiSessionProvider, AiSessionReaderContext } from "@/lib/aiSessions/types";

export async function aggregateAiSessions(context: AiSessionReaderContext): Promise<AggregatedAiSessionsResult> {
  const [claude, codex, openCode, gemini] = await Promise.all([
    readClaudeSessions(context),
    readCodexSessions(context),
    readOpenCodeSessions(context),
    readGeminiSessions(context),
  ]);

  return createAggregationResult({
    targetPath: context.worktreePath,
    repoPath: context.repoPath,
    sessions: sortSessionsDescending([...claude.sessions, ...codex.sessions, ...openCode.sessions, ...gemini.sessions]),
    sources: [claude, codex, openCode, gemini].map(toSourceStatus),
  });
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
      return readGeminiSessionDetail(context, sessionId);
    default:
      return null;
  }
}
