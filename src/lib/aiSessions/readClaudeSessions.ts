import { access, readdir } from "fs/promises";
import { homedir } from "os";
import path from "path";
import {
  createReaderResult,
  createSessionDetail,
  determineMatchScope,
  extractPlainText,
  getCachedOrParse,
  getCachedOrParseHead,
  getCandidatePaths,
  makePreviewMessage,
  paginateItems,
  readJsonLines,
  readJsonLinesHead,
  sortMessagesDescending,
  toIsoString,
  truncateText,
} from "@/lib/aiSessions/shared";
import type {
  AggregatedAiMessage,
  AggregatedAiSession,
  AiMessageRole,
  AiSessionDetailReaderResult,
  AiSessionReaderContext,
  AiSessionReaderResult,
} from "@/lib/aiSessions/types";

const CLAUDE_ROOT_DIR = path.join(homedir(), ".claude");
const CLAUDE_PROJECTS_DIR = path.join(CLAUDE_ROOT_DIR, "projects");
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
}

export async function readClaudeSessions(context: AiSessionReaderContext): Promise<AiSessionReaderResult> {
  const rootExists = await pathExists(CLAUDE_ROOT_DIR);
  if (!rootExists) {
    return createReaderResult("claude", { available: false, reason: "Claude Code directory not found" });
  }

  const projectFiles = await findProjectFiles(context);
  if (projectFiles.length === 0) {
    return createReaderResult("claude", { sessions: [], reason: "No Claude project session files matched this task" });
  }

  const results = await Promise.all(
    projectFiles.map((filePath) => parseClaudeSessionFromFile(filePath, context))
  );

  return createReaderResult("claude", {
    sessions: results.filter((s): s is AggregatedAiSession => s !== null),
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
    const events = await getCachedOrParse(filePath, () => readJsonLines(filePath));
    for (const rawEvent of events) {
      const event = rawEvent as ClaudeProjectEvent;
      if (event.sessionId !== sessionId) continue;
      if (typeof event.cwd === "string" && !determineMatchScope(event.cwd, context)) continue;
      if (!matchedPath && typeof event.cwd === "string") {
        matchedPath = event.cwd;
      }

      const role = resolveClaudeRole(event);
      const text = extractPlainText(event.message?.content);
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
    return first?.session ?? null;
  };

  const headEvents = await getCachedOrParseHead(
    filePath,
    () => readJsonLinesHead(filePath, 60)
  );

  const headResult = await parseEvents(headEvents);
  if (headResult?.firstUserPrompt) return headResult;

  const allEvents = await getCachedOrParse(filePath, () => readJsonLines(filePath));
  return parseEvents(allEvents);
}

async function findProjectFiles(context: AiSessionReaderContext): Promise<string[]> {
  const candidateDirs = getCandidatePaths(context)
    .map(toClaudeProjectDirName)
    .map((directoryName) => path.join(CLAUDE_PROJECTS_DIR, directoryName));

  const uniqueDirs = Array.from(new Set(candidateDirs));
  const files: string[] = [];

  for (const directoryPath of uniqueDirs) {
    if (!(await pathExists(directoryPath))) continue;
    const entries = await readdir(directoryPath, { withFileTypes: true });
    files.push(
      ...entries
        .filter((entry) => entry.isFile() && entry.name.endsWith(".jsonl"))
        .map((entry) => path.join(directoryPath, entry.name))
    );
  }

  return files;
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

  const accumulator = getOrCreateClaudeSession(sessions, sessionId, cwd, matchScope, event.timestamp, sourceRef);
  const role = resolveClaudeRole(event);
  const text = extractPlainText(event.message?.content);

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

  const accumulator = { session };
  sessions.set(sessionId, accumulator);
  return accumulator;
}

function resolveClaudeRole(event: ClaudeProjectEvent): AiMessageRole {
  if (event.message?.role === "user") return "user";
  if (event.message?.role === "assistant") return "assistant";
  if (event.type === "system") return "system";
  if (event.type === "progress") return "tool";
  return "unknown";
}

async function pathExists(targetPath: string): Promise<boolean> {
  return access(targetPath).then(() => true).catch(() => false);
}
