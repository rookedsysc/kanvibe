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
  pickLatestFile,
  readJsonLines,
  readJsonLinesHead,
  readJsonLinesTail,
  REMOTE_SESSION_FILE_PARSE_CONCURRENCY,
  sortMessagesDescending,
  toIsoString,
  truncateText,
} from "@/lib/aiSessions/shared";
import {
  getHomeDirectory,
  listFilesModifiedWithin,
  listFilesRecursivelyBySuffix,
  pathExists,
  readDirectoryFilesBySuffix,
} from "@/lib/hostFileAccess";
import type {
  AggregatedAiMessage,
  AggregatedAiSession,
  AiMessageRole,
  AiSessionDetailReaderResult,
  AiSessionReaderContext,
  AiSessionReaderResult,
  LiveAiSessionWindows,
  LiveAiSubtask,
  LiveProviderSnapshot,
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

  const uniqueFiles = Array.from(new Set(files));
  if (uniqueFiles.length > 0) {
    return uniqueFiles;
  }

  // Claude Code stores the canonical cwd inside each JSONL event, but the outer
  // ~/.claude/projects/<encoded-path> directory can diverge when paths are
  // symlinked, migrated, or encoded by a different CLI version. If the direct
  // encoded-directory lookup misses, scan project JSONL files and let cwd-based
  // parsing decide which sessions belong to this task/repo.
  return Array.from(new Set(
    await listFilesRecursivelyBySuffix(claudeProjectsDirectory, ".jsonl", context.sshHost),
  ));
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
  matchScope: AggregatedAiSession["matchScope"],
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

/**
 * 이 worktree에서 가장 최근에 움직인 Claude 세션과, 지금 돌고 있는 서브에이전트를 찾는다.
 *
 * 세션 본문은 읽지 않는다. 세션 파일 이름이 곧 세션 id이고 서브에이전트는 세션 id 아래 별도 디렉터리에
 * 파일로 떨어지므로, 수정 시각만 보면 폴링 비용 없이 실행 여부와 개수를 알 수 있다.
 * 서브에이전트 이름만 첫 줄을 읽어 채우는데, 그 줄에 위임받은 작업 프롬프트가 들어 있기 때문이다.
 */
export async function readClaudeLiveSession(
  context: AiSessionReaderContext,
  windows: LiveAiSessionWindows,
): Promise<LiveProviderSnapshot | null> {
  const projectsDirectory = await getClaudeProjectsDirectory(context);
  const candidateDirectories = getCandidatePaths(context)
    .map(toClaudeProjectDirName)
    .map((directoryName) => path.join(projectsDirectory, directoryName));

  const recentFiles = (await Promise.all(candidateDirectories.map(async (directory) => {
    const files = await listFilesModifiedWithin(directory, ".jsonl", windows.recentWindowMs, context.sshHost);
    // 서브에이전트 기록도 같은 트리 아래 있으므로, 세션 파일은 디렉터리 바로 밑에 있는 것만 본다
    return files.filter((file) => path.dirname(file.filePath) === directory);
  }))).flat();

  const latestSessionFile = pickLatestFile(recentFiles);
  if (!latestSessionFile) {
    return null;
  }

  const sessionId = path.basename(latestSessionFile.filePath, ".jsonl");
  const subagentsDirectory = path.join(path.dirname(latestSessionFile.filePath), sessionId, "subagents");
  const runningSubagentFiles = await listFilesModifiedWithin(
    subagentsDirectory,
    ".jsonl",
    windows.runningWindowMs,
    context.sshHost,
  );

  return {
    sessionId,
    currentTask: await readCurrentActivity(latestSessionFile.filePath, context.sshHost),
    lastActiveAt: new Date(latestSessionFile.mtimeMs).toISOString(),
    runningSubtasks: await Promise.all(runningSubagentFiles.map((file) =>
      toClaudeSubtask(file, context.sshHost))),
  };
}

/**
 * 세션이 지금 무엇을 하는지는 마지막 AI 응답이 가장 잘 말해준다.
 * 사용자 요청은 "무엇을 시켰나"이고 AI 응답은 "지금 무엇을 하는 중인가"라 화면 목적에 더 맞는다.
 * 아직 응답이 없는 갓 시작한 세션은 마지막 사용자 요청으로 되돌린다.
 */
async function readCurrentActivity(
  filePath: string,
  sshHost: string | null | undefined,
): Promise<string | null> {
  try {
    const events = await readJsonLinesTail(filePath, CLAUDE_TAIL_EVENT_COUNT, sshHost);
    return findLatestRoleText(events, "assistant") ?? findLatestRoleText(events, "user");
  } catch {
    // 세션 기록을 읽지 못해도 실행중 여부는 파일 활동만으로 판정할 수 있다.
    return null;
  }
}

function findLatestRoleText(events: unknown[], role: "assistant" | "user"): string | null {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index] as ClaudeProjectEvent;
    if (event?.message?.role !== role) continue;

    const text = extractPlainText(event.message.content);
    if (text) {
      return truncateText(text, CLAUDE_CURRENT_TASK_LENGTH);
    }
  }

  return null;
}

const CLAUDE_TAIL_EVENT_COUNT = 60;
const CLAUDE_CURRENT_TASK_LENGTH = 80;

/** `agent-<agentId>.jsonl`의 첫 줄에는 위임받은 작업 프롬프트가 들어 있어, 이름 대신 그 앞부분을 보여준다 */
async function toClaudeSubtask(
  file: { filePath: string; mtimeMs: number },
  sshHost: string | null | undefined,
): Promise<LiveAiSubtask> {
  const agentId = path.basename(file.filePath, ".jsonl").replace(/^agent-/, "");

  return {
    id: agentId,
    name: await readClaudeSubagentTaskLabel(file.filePath, sshHost),
    lastActiveAt: new Date(file.mtimeMs).toISOString(),
  };
}

async function readClaudeSubagentTaskLabel(
  filePath: string,
  sshHost: string | null | undefined,
): Promise<string | null> {
  try {
    const [firstEvent] = await readJsonLinesHead(filePath, 1, sshHost);
    const promptText = extractPlainText((firstEvent as ClaudeProjectEvent | undefined)?.message?.content);
    return promptText ? truncateText(promptText, CLAUDE_SUBTASK_LABEL_LENGTH) : null;
  } catch {
    return null;
  }
}

const CLAUDE_SUBTASK_LABEL_LENGTH = 40;

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
