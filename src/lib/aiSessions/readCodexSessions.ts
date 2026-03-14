import { access } from "fs/promises";
import { homedir } from "os";
import path from "path";
import {
  createReaderResult,
  createSessionDetail,
  determineMatchScope,
  extractPlainText,
  getCachedOrParse,
  listFilesRecursively,
  makePreviewMessage,
  paginateItems,
  readJsonLines,
  sortMessagesDescending,
  toIsoString,
  truncateText,
} from "@/lib/aiSessions/shared";
import type { AggregatedAiMessage, AggregatedAiSession, AiSessionDetailReaderResult, AiSessionReaderContext, AiSessionReaderResult } from "@/lib/aiSessions/types";

const CODEX_ROOT_DIR = path.join(homedir(), ".codex");
const CODEX_SESSIONS_DIR = path.join(CODEX_ROOT_DIR, "sessions");
const DEFAULT_DETAIL_LIMIT = 20;

interface CodexRolloutEvent {
  type?: string;
  timestamp?: string;
  payload?: Record<string, unknown>;
}

export async function readCodexSessions(context: AiSessionReaderContext): Promise<AiSessionReaderResult> {
  const sessionsDirExists = await access(CODEX_SESSIONS_DIR).then(() => true).catch(() => false);
  if (!sessionsDirExists) {
    return createReaderResult("codex", { available: false, reason: "Codex sessions directory not found" });
  }

  const rolloutFiles = await listFilesRecursively(CODEX_SESSIONS_DIR, (filePath) => filePath.endsWith(".jsonl"));

  const results = await Promise.all(
    rolloutFiles.map((filePath) => parseCodexSessionSummary(filePath, context))
  );
  const sessions = results.filter((s): s is AggregatedAiSession => s !== null);

  return createReaderResult("codex", {
    sessions,
    reason: sessions.length === 0 ? "No Codex sessions matched this task" : null,
  });
}

export async function readCodexSessionDetail(
  context: AiSessionReaderContext,
  sessionId: string,
  sourceRef?: string | null,
  cursor?: string | null,
  limit = DEFAULT_DETAIL_LIMIT
): Promise<AiSessionDetailReaderResult | null> {
  const rolloutFiles = sourceRef
    ? [sourceRef]
    : await listFilesRecursively(CODEX_SESSIONS_DIR, (filePath) => filePath.endsWith(".jsonl"));

  for (const filePath of rolloutFiles) {
    const detail = await parseCodexSessionDetail(filePath, context, sessionId, cursor, limit);
    if (detail) return detail;
  }

  return null;
}

async function parseCodexSessionSummary(filePath: string, context: AiSessionReaderContext): Promise<AggregatedAiSession | null> {
  const events = await getCachedOrParse(filePath, () => readJsonLines(filePath));

  let sessionId: string | null = null;
  let matchedPath: string | null = null;
  let matchScope: AggregatedAiSession["matchScope"] | null = null;
  let startedAt: string | null = null;
  let updatedAt: string | null = null;
  let firstUserPrompt: string | null = null;
  let messageCount = 0;

  for (const rawEvent of events) {
    const event = rawEvent as CodexRolloutEvent;
    if (event.type === "session_meta") {
      const payload = event.payload ?? {};
      const cwd = typeof payload.cwd === "string" ? payload.cwd : null;
      const resolvedMatchScope = determineMatchScope(cwd, context);
      if (!resolvedMatchScope) return null;

      sessionId = typeof payload.id === "string" ? payload.id : path.basename(filePath, ".jsonl");
      matchedPath = cwd;
      matchScope = resolvedMatchScope;
      startedAt = toIsoString(payload.timestamp ?? event.timestamp);
      updatedAt = toIsoString(payload.timestamp ?? event.timestamp);
      continue;
    }

    if (event.type !== "response_item") continue;
    const payload = event.payload ?? {};
    if (payload.type !== "message") continue;

    const text = extractPlainText(payload.content);
    if (!text) continue;

    if (payload.role === "user" && !firstUserPrompt) {
      firstUserPrompt = text;
    }

    messageCount += 1;
    updatedAt = toIsoString(event.timestamp) ?? updatedAt;
  }

  if (!sessionId || !matchedPath || !matchScope) return null;

  return {
    id: sessionId,
    provider: "codex",
    startedAt,
    updatedAt,
    matchedPath,
    matchScope,
    title: firstUserPrompt ? truncateText(firstUserPrompt, 80) : null,
    firstUserPrompt: firstUserPrompt ? truncateText(firstUserPrompt) : null,
    messageCount,
    sourceRef: filePath,
  };
}

async function parseCodexSessionDetail(
  filePath: string,
  context: AiSessionReaderContext,
  sessionId: string,
  cursor?: string | null,
  limit = DEFAULT_DETAIL_LIMIT
): Promise<AiSessionDetailReaderResult | null> {
  const events = await getCachedOrParse(filePath, () => readJsonLines(filePath));
  let matchedPath: string | null = null;
  let title: string | null = null;
  const messages: AggregatedAiMessage[] = [];

  for (const rawEvent of events) {
    const event = rawEvent as CodexRolloutEvent;
    if (event.type === "session_meta") {
      const payload = event.payload ?? {};
      const candidateSessionId = typeof payload.id === "string" ? payload.id : path.basename(filePath, ".jsonl");
      if (candidateSessionId !== sessionId) return null;

      const cwd = typeof payload.cwd === "string" ? payload.cwd : null;
      if (!determineMatchScope(cwd, context)) return null;
      matchedPath = cwd;
      continue;
    }

    if (event.type !== "response_item") continue;
    const payload = event.payload ?? {};
    if (payload.type !== "message") continue;

    const role = payload.role === "user" || payload.role === "assistant" ? payload.role : "unknown";
    const text = extractPlainText(payload.content);
    if (!text) continue;

    if (role === "user" && !title) {
      title = truncateText(text, 80);
    }

    const previewMessage = makePreviewMessage(role, event.timestamp, text);
    if (previewMessage) messages.push(previewMessage);
  }

  if (!matchedPath) return null;
  const paginated = paginateItems(sortMessagesDescending(messages), cursor, limit);
  return createSessionDetail({
    sessionId,
    provider: "codex",
    title,
    matchedPath,
    sourceRef: filePath,
    messages: paginated.items,
    nextCursor: paginated.nextCursor,
  });
}
