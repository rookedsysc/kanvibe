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

  let allSessions = [...claude.sessions, ...codex.sessions, ...openCode.sessions, ...gemini.sessions];

  // 각 리더에서 이미 필터링하지만, 취합 단계에서 다시 한 번 필터링 (선택 사항이나 일관성 위해 유지)
  if (context.query) {
    const q = context.query.toLowerCase();
    allSessions = allSessions.filter((s) =>
      s.title?.toLowerCase().includes(q) ||
      s.firstUserPrompt?.toLowerCase().includes(q) ||
      s.matchedPath?.toLowerCase().includes(q)
    );
  }

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
      return readGeminiSessionDetail(context, sessionId);
    default:
      return null;
  }
}
