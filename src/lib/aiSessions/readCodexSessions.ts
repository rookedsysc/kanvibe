import path from "path";
import {
  createReaderResult,
  createSessionDetail,
  determineMatchScope,
  extractPlainText,
  getCachedOrParse,
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
} from "@/lib/hostFileAccess";
import type {
  AggregatedAiMessage,
  AggregatedAiSession,
  AiMessageRole,
  AiSessionDetailReaderResult,
  AiSessionReaderContext,
  AiSessionReaderResult,
  LiveAiSessionWindows,
  LiveProviderSnapshot,
} from "@/lib/aiSessions/types";

const DEFAULT_DETAIL_LIMIT = 20;

interface CodexRolloutEvent {
  type?: string;
  timestamp?: string;
  payload?: Record<string, unknown>;
}

export async function readCodexSessions(context: AiSessionReaderContext): Promise<AiSessionReaderResult> {
  const codexSessionsDirectory = await getCodexSessionsDirectory(context);
  const sessionsDirExists = await pathExists(codexSessionsDirectory, context.sshHost);
  if (!sessionsDirExists) {
    return createReaderResult("codex", { available: false, reason: "Codex sessions directory not found" });
  }

  const rolloutFiles = await listFilesRecursivelyBySuffix(codexSessionsDirectory, ".jsonl", context.sshHost);
  const parseConcurrency = context.sshHost ? REMOTE_SESSION_FILE_PARSE_CONCURRENCY : rolloutFiles.length || 1;

  const results = await mapWithConcurrency(
    rolloutFiles,
    parseConcurrency,
    (filePath) => parseCodexSessionSummary(filePath, context),
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
    : await listFilesRecursivelyBySuffix(await getCodexSessionsDirectory(context), ".jsonl", context.sshHost);

  for (const filePath of rolloutFiles) {
    const detail = await parseCodexSessionDetail(filePath, context, sessionId, cursor, limit);
    if (detail) return detail;
  }

  return null;
}

async function parseCodexSessionSummary(filePath: string, context: AiSessionReaderContext): Promise<AggregatedAiSession | null> {
  const events = await getCachedOrParse(filePath, () => readJsonLines(filePath, context.sshHost), context.sshHost);

  let sessionId: string | null = null;
  let matchedPath: string | null = null;
  let matchScope: AggregatedAiSession["matchScope"] | null = null;
  let startedAt: string | null = null;
  let updatedAt: string | null = null;
  let firstUserPrompt: string | null = null;
  let messageCount = 0;
  let hasQueryMatch = false;
  const seenMessages = new Set<string>();

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
      if (matchesCodexQuery(context.query, cwd, sessionId)) {
        hasQueryMatch = true;
      }
      continue;
    }

    const parsedMessages = extractCodexEventMessages(event);
    for (const message of parsedMessages) {
      if (shouldSkipCodexMessage(message.role, message.text)) {
        continue;
      }
      const messageKey = createCodexMessageKey(message.role, event.timestamp, message.text);
      if (seenMessages.has(messageKey)) {
        continue;
      }
      seenMessages.add(messageKey);

      if (message.role === "user" && !firstUserPrompt) {
        firstUserPrompt = message.text;
      }

      if (matchesCodexQuery(context.query, message.text)) {
        hasQueryMatch = true;
      }

      messageCount += 1;
      updatedAt = toIsoString(event.timestamp) ?? updatedAt;
    }
  }

  if (!sessionId || !matchedPath || !matchScope) return null;
  if (context.query && !matchesCodexQuery(context.query, firstUserPrompt, matchedPath, sessionId) && !hasQueryMatch) return null;

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
  const events = await getCachedOrParse(filePath, () => readJsonLines(filePath, context.sshHost), context.sshHost);
  let matchedPath: string | null = null;
  let title: string | null = null;
  const messages: AggregatedAiMessage[] = [];
  const seenMessages = new Set<string>();

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

    for (const message of extractCodexEventMessages(event)) {
      if (shouldSkipCodexMessage(message.role, message.text)) {
        continue;
      }
      const messageKey = createCodexMessageKey(message.role, event.timestamp, message.text);
      if (seenMessages.has(messageKey)) {
        continue;
      }
      seenMessages.add(messageKey);

      if (context.roles && context.roles.length > 0 && !context.roles.includes(message.role)) {
        continue;
      }

      if (context.query && !message.text.toLowerCase().includes(context.query.toLowerCase())) {
        continue;
      }

      if (message.role === "user" && !title) {
        title = truncateText(message.text, 80);
      }

      const previewMessage = makePreviewMessage(message.role, event.timestamp, message.text);
      if (previewMessage) messages.push(previewMessage);
    }
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

function extractCodexEventMessages(event: CodexRolloutEvent): Array<{ role: AiMessageRole; text: string }> {
  if (event.type === "response_item") {
    return extractCodexPayloadMessages(event.payload ?? {});
  }

  if (event.type === "event_msg") {
    const payload = event.payload ?? {};
    if (payload.type === "agent_message") {
      const text = typeof payload.message === "string"
        ? payload.message
        : extractPlainText(payload.message ?? payload);
      return text ? [{ role: "assistant", text }] : [];
    }
  }

  return [];
}

function createCodexMessageKey(role: AiMessageRole, timestamp: string | undefined, text: string): string {
  return `${role}\u0000${timestamp ?? ""}\u0000${text}`;
}

function shouldSkipCodexMessage(role: AiMessageRole, text: string): boolean {
  const trimmed = text.trim();

  if (role === "user") {
    return [
      /^<environment_context>/,
      /^<skill>/,
      /^<system-reminder>/,
    ].some((pattern) => pattern.test(trimmed));
  }

  if (role === "developer") {
    return [
      /<permissions instructions>/,
      /<apps_instructions>/,
      /<skills_instructions>/,
      /<collaboration_mode>/,
    ].some((pattern) => pattern.test(trimmed));
  }

  return false;
}

function extractCodexPayloadMessages(payload: Record<string, unknown>): Array<{ role: AiMessageRole; text: string }> {
  if (payload.type === "message") {
    const role = resolveCodexMessageRole(payload.role);
    const text = extractPlainText(payload.content);
    return text ? [{ role, text }] : [];
  }

  if (payload.type === "reasoning") {
    const text = extractPlainText(payload.summary ?? payload.content ?? payload.text ?? payload);
    return text ? [{ role: "reasoning", text }] : [];
  }

  if (payload.type === "function_call" || payload.type === "tool_call") {
    const name = typeof payload.name === "string"
      ? payload.name
      : typeof payload.call_id === "string"
        ? payload.call_id
        : "tool";
    const argumentText = extractPlainText(payload.arguments ?? payload.args ?? payload.input);
    const text = argumentText ? `${name}: ${argumentText}` : `${name} called`;
    return [{ role: "tool", text }];
  }

  if (payload.type === "function_call_output" || payload.type === "tool_result") {
    const text = extractPlainText(payload.output ?? payload.result ?? payload.content ?? payload);
    return text ? [{ role: "tool", text }] : [];
  }

  const text = extractPlainText(payload);
  return text ? [{ role: "unknown", text }] : [];
}

/**
 * 이 worktree에서 마지막으로 움직인 Codex 스레드와, 그 스레드가 띄운 서브에이전트를 찾는다.
 *
 * Codex는 서브에이전트도 자기 rollout 파일을 따로 만들고 첫 줄 `session_meta`에 부모 스레드를 남긴다.
 * 그래서 최근에 수정된 rollout만 골라 첫 줄만 읽으면 부모·자식 관계를 전부 복원할 수 있다.
 */
export async function readCodexLiveSession(
  context: AiSessionReaderContext,
  windows: LiveAiSessionWindows,
): Promise<LiveProviderSnapshot | null> {
  const recentFiles = await listFilesModifiedWithin(
    await getCodexSessionsDirectory(context),
    ".jsonl",
    windows.recentWindowMs,
    context.sshHost,
  );

  const metaByFile = await mapWithConcurrency(
    recentFiles,
    context.sshHost ? REMOTE_SESSION_FILE_PARSE_CONCURRENCY : recentFiles.length || 1,
    async (file) => ({ file, meta: await readCodexSessionMeta(file.filePath, context.sshHost) }),
  );

  const threads = metaByFile.filter((entry): entry is { file: typeof entry.file; meta: CodexThreadMeta } =>
    entry.meta !== null);

  const latestOwnThread = pickLatestFile(threads
    .filter((thread) => !thread.meta.parentThreadId && determineMatchScope(thread.meta.cwd, context))
    .map((thread) => ({ ...thread.file, meta: thread.meta })));

  if (!latestOwnThread) {
    return null;
  }

  const runningSince = Date.now() - windows.runningWindowMs;

  return {
    sessionId: latestOwnThread.meta.threadId,
    currentTask: await readLatestCodexUserRequest(latestOwnThread.filePath, context.sshHost),
    lastActiveAt: new Date(latestOwnThread.mtimeMs).toISOString(),
    runningSubtasks: threads
      .filter((thread) => thread.meta.parentThreadId === latestOwnThread.meta.threadId)
      .filter((thread) => thread.file.mtimeMs >= runningSince)
      .map((thread) => ({
        id: thread.meta.threadId,
        name: thread.meta.agentNickname,
        lastActiveAt: new Date(thread.file.mtimeMs).toISOString(),
      })),
  };
}

/** 세션이 지금 붙들고 있는 작업. rollout 꼬리에서 마지막 사용자 입력을 찾는다 */
async function readLatestCodexUserRequest(
  filePath: string,
  sshHost: string | null | undefined,
): Promise<string | null> {
  try {
    const events = await readJsonLinesTail(filePath, CODEX_TAIL_EVENT_COUNT, sshHost);
    for (let index = events.length - 1; index >= 0; index -= 1) {
      const payload = (events[index] as CodexRolloutEvent)?.payload;
      if (payload?.role !== "user") continue;

      const text = extractPlainText(payload.content);
      if (text) {
        return truncateText(text, CODEX_CURRENT_TASK_LENGTH);
      }
    }
  } catch {
    // rollout을 읽지 못해도 실행중 여부는 파일 활동만으로 판정할 수 있다.
  }

  return null;
}

const CODEX_TAIL_EVENT_COUNT = 60;
const CODEX_CURRENT_TASK_LENGTH = 80;

interface CodexThreadMeta {
  threadId: string;
  parentThreadId: string | null;
  agentNickname: string | null;
  cwd: string | null;
}

/** rollout 첫 줄의 `session_meta`만 읽는다. 본문까지 파싱하면 폴링마다 전체 대화를 훑게 된다 */
async function readCodexSessionMeta(
  filePath: string,
  sshHost: string | null | undefined,
): Promise<CodexThreadMeta | null> {
  try {
    const [firstLine] = await readJsonLinesHead(filePath, 1, sshHost);
    const payload = (firstLine as { type?: string; payload?: Record<string, unknown> } | undefined);
    if (payload?.type !== "session_meta" || !payload.payload) {
      return null;
    }

    const meta = payload.payload;
    const threadId = typeof meta.id === "string"
      ? meta.id
      : typeof meta.session_id === "string" ? meta.session_id : null;

    if (!threadId) {
      return null;
    }

    return {
      threadId,
      parentThreadId: typeof meta.parent_thread_id === "string" ? meta.parent_thread_id : null,
      agentNickname: typeof meta.agent_nickname === "string" ? meta.agent_nickname : null,
      cwd: typeof meta.cwd === "string" ? meta.cwd : null,
    };
  } catch {
    return null;
  }
}

function matchesCodexQuery(query: string | undefined, ...values: Array<string | null | undefined>): boolean {
  if (!query) return true;
  const normalizedQuery = query.toLowerCase();
  return values.some((value) => value?.toLowerCase().includes(normalizedQuery));
}

function resolveCodexMessageRole(role: unknown): AiMessageRole {
  if (role === "user") return "user";
  if (role === "assistant") return "assistant";
  if (role === "system") return "system";
  if (role === "developer") return "developer";
  if (role === "tool") return "tool";
  return "unknown";
}

async function getCodexSessionsDirectory(context: AiSessionReaderContext): Promise<string> {
  return path.join(await getHomeDirectory(context.sshHost), ".codex", "sessions");
}
