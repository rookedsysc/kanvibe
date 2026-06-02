import path from "path";
import {
  createReaderResult,
  createSessionDetail,
  determineMatchScope,
  extractPlainText,
  getCachedOrParse,
  getCachedOrParseHead,
  getCandidatePaths,
  mapWithConcurrency,
  makePreviewMessage,
  paginateItems,
  readJsonLines,
  readJsonLinesHead,
  REMOTE_SESSION_FILE_PARSE_CONCURRENCY,
  sortMessagesDescending,
  toIsoString,
  truncateText,
} from "@/lib/aiSessions/shared";
import { getHomeDirectory, pathExists, readDirectoryFilesBySuffix } from "@/lib/hostFileAccess";
import type {
  AggregatedAiMessage,
  AggregatedAiSession,
  AiMessageRole,
  AiSessionDetailReaderResult,
  AiSessionReaderContext,
  AiSessionReaderResult,
} from "@/lib/aiSessions/types";

const DEFAULT_DETAIL_LIMIT = 20;

interface ClaudeProjectEvent {
  type?: string;
  timestamp?: string;
  sessionId?: string;
  cwd?: string;
  message?: {
    role?: string;
    content?: unknown;
  };
}

interface ClaudeSessionAccumulator {
  session: AggregatedAiSession;
  hasQueryMatch: boolean;
}

export async function readClaudeSessions(context: AiSessionReaderContext): Promise<AiSessionReaderResult> {
  const rootExists = await pathExists(await getClaudeRootDirectory(context), context.sshHost);
  if (!rootExists) {
    return createReaderResult("claude", { available: false, reason: "Claude Code directory not found" });
  }

  const projectFiles = await findProjectFiles(context);
  if (projectFiles.length === 0) {
    return createReaderResult("claude", { sessions: [], reason: "No Claude project session files matched this task" });
  }

  const parseConcurrency = context.sshHost ? REMOTE_SESSION_FILE_PARSE_CONCURRENCY : projectFiles.length || 1;
  const results = await mapWithConcurrency(
    projectFiles,
    parseConcurrency,
    (filePath) => parseClaudeSessionFromFile(filePath, context),
  );

  const sessions = results.filter((s): s is AggregatedAiSession => s !== null);

  return createReaderResult("claude", {
    sessions,
  });
}

export async function readClaudeSessionDetail(
  context: AiSessionReaderContext,
  sessionId: string,
  sourceRef?: string | null,
  cursor?: string | null,
  limit = DEFAULT_DETAIL_LIMIT
): Promise<AiSessionDetailReaderResult | null> {
  const projectFiles = sourceRef ? [sourceRef] : await findProjectFiles(context);
  if (projectFiles.length === 0) return null;

  let title: string | null = null;
  let matchedPath: string | null = null;
  const messages: AggregatedAiMessage[] = [];

  for (const filePath of projectFiles) {
    const events = await getCachedOrParse(filePath, () => readJsonLines(filePath, context.sshHost), context.sshHost);
    for (const rawEvent of events) {
      const event = rawEvent as ClaudeProjectEvent;
      if (event.sessionId !== sessionId) continue;
      if (typeof event.cwd === "string" && !determineMatchScope(event.cwd, context)) continue;
      if (!matchedPath && typeof event.cwd === "string") {
        matchedPath = event.cwd;
      }

      const role = resolveClaudeRole(event);
      if (context.roles && context.roles.length > 0 && !context.roles.includes(role)) {
        continue;
      }

      const text = extractPlainText(event.message?.content);
      if (context.query && text && !text.toLowerCase().includes(context.query.toLowerCase())) {
        continue;
      }

      if (role === "user" && text && !title) {
        title = truncateText(text, 80);
      }

      const previewMessage = makePreviewMessage(role, event.timestamp, text);
      if (previewMessage) {
        messages.push(previewMessage);
      }
    }
  }

  if (!matchedPath && messages.length === 0) return null;

  const paginated = paginateItems(sortMessagesDescending(messages), cursor, limit);
  return createSessionDetail({
    sessionId,
    provider: "claude",
    title,
    matchedPath,
    sourceRef: sourceRef ?? null,
    messages: paginated.items,
    nextCursor: paginated.nextCursor,
  });
}

/** 단일 JSONL 파일에서 세션 메타데이터를 추출한다. 앞 60줄로 충분하면 조기 종료한다. */
async function parseClaudeSessionFromFile(
  filePath: string,
  context: AiSessionReaderContext
): Promise<AggregatedAiSession | null> {
  const parseEvents = async (events: unknown[]): Promise<AggregatedAiSession | null> => {
    const accumulator = new Map<string, ClaudeSessionAccumulator>();
    for (const rawEvent of events) {
      consumeClaudeListEvent(accumulator, rawEvent as ClaudeProjectEvent, context, filePath);
    }
    const first = Array.from(accumulator.values())[0];
    if (!first) return null;
    if (context.query && !first.hasQueryMatch) return null;
    return first.session;
  };

  if (context.query) {
    const allEvents = await getCachedOrParse(filePath, () => readJsonLines(filePath, context.sshHost), context.sshHost);
    return parseEvents(allEvents);
  }

  const headEvents = await getCachedOrParseHead(
    filePath,
    () => readJsonLinesHead(filePath, 60, context.sshHost),
    context.sshHost,
  );

  const headResult = await parseEvents(headEvents);
  if (headResult?.firstUserPrompt) return headResult;

  const allEvents = await getCachedOrParse(filePath, () => readJsonLines(filePath, context.sshHost), context.sshHost);
  return parseEvents(allEvents);
}

async function findProjectFiles(context: AiSessionReaderContext): Promise<string[]> {
  const claudeProjectsDirectory = await getClaudeProjectsDirectory(context);
  const candidateDirs = getCandidatePaths(context)
    .map(toClaudeProjectDirName)
    .map((directoryName) => path.join(claudeProjectsDirectory, directoryName));

  const uniqueDirs = Array.from(new Set(candidateDirs));
  const files: string[] = [];

  for (const directoryPath of uniqueDirs) {
    files.push(...await readDirectoryFilesBySuffix(directoryPath, ".jsonl", context.sshHost));
  }

  return files;
}

async function getClaudeRootDirectory(context: AiSessionReaderContext): Promise<string> {
  return path.join(await getHomeDirectory(context.sshHost), ".claude");
}

async function getClaudeProjectsDirectory(context: AiSessionReaderContext): Promise<string> {
  return path.join(await getClaudeRootDirectory(context), "projects");
}

/** Claude Code는 프로젝트 디렉토리 이름을 생성할 때 경로 구분자(/)와 언더스코어(_) 모두 하이픈(-)으로 치환한다 */
function toClaudeProjectDirName(targetPath: string): string {
  return path.resolve(targetPath).replaceAll(path.sep, "-").replaceAll("_", "-");
}

function consumeClaudeListEvent(
  sessions: Map<string, ClaudeSessionAccumulator>,
  event: ClaudeProjectEvent,
  context: AiSessionReaderContext,
  sourceRef: string
): void {
  const sessionId = typeof event.sessionId === "string" ? event.sessionId : null;
  const cwd = typeof event.cwd === "string" ? event.cwd : null;
  const matchScope = determineMatchScope(cwd, context);
  if (!sessionId || !cwd || !matchScope) return;

  const role = resolveClaudeRole(event);
  const text = extractPlainText(event.message?.content);

  const accumulator = getOrCreateClaudeSession(sessions, sessionId, cwd, matchScope, event.timestamp, sourceRef);

  if (role === "user" && text && !accumulator.session.firstUserPrompt) {
    accumulator.session.firstUserPrompt = text;
    accumulator.session.title = truncateText(text, 80);
  }

  if (text) {
    accumulator.session.messageCount += 1;
  }

  const normalizedTimestamp = toIsoString(event.timestamp);
  if (normalizedTimestamp) {
    accumulator.session.updatedAt = normalizedTimestamp;
  }

  if (matchesClaudeQuery(context.query, text, accumulator.session.title, accumulator.session.firstUserPrompt, accumulator.session.matchedPath)) {
    accumulator.hasQueryMatch = true;
  }
}

function getOrCreateClaudeSession(
  sessions: Map<string, ClaudeSessionAccumulator>,
  sessionId: string,
  cwd: string,
  matchScope: "worktree" | "repo" | "unknown",
  timestamp: string | undefined,
  sourceRef: string
): ClaudeSessionAccumulator {
  const existing = sessions.get(sessionId);
  if (existing) return existing;

  const session: AggregatedAiSession = {
    id: sessionId,
    provider: "claude",
    startedAt: toIsoString(timestamp),
    updatedAt: toIsoString(timestamp),
    matchedPath: cwd,
    matchScope,
    title: null,
    firstUserPrompt: null,
    messageCount: 0,
    sourceRef,
  };

  const accumulator = { session, hasQueryMatch: false };
  sessions.set(sessionId, accumulator);
  return accumulator;
}

function matchesClaudeQuery(query: string | undefined, ...values: Array<string | null | undefined>): boolean {
  if (!query) return true;
  const normalizedQuery = query.toLowerCase();
  return values.some((value) => value?.toLowerCase().includes(normalizedQuery));
}

function resolveClaudeRole(event: ClaudeProjectEvent): AiMessageRole {
  if (hasClaudeContentPartType(event.message?.content, "tool_result")) return "tool";
  if (event.message?.role === "system" || event.type === "system") return "system";
  if (event.message?.role === "user") return "user";
  if (event.message?.role === "assistant") return "assistant";
  if (event.type === "progress") return "tool";
  return "unknown";
}

function hasClaudeContentPartType(content: unknown, partType: string): boolean {
  if (!Array.isArray(content)) {
    return false;
  }

  return content.some((part) => Boolean(
    part &&
    typeof part === "object" &&
    (part as Record<string, unknown>).type === partType,
  ));
}
