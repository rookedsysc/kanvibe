import path from "path";
import { extractMessageAttachments } from "./messageAttachments";
import { summarizeCodexCustomToolCall, withToolCall } from "./messageFiles";
import {
  createReaderResult,
  createSessionDetail,
  determineMatchScope,
  extractPlainText,
  getCachedOrParse,
  mapWithConcurrency,
  makePreviewMessage,
  LOCAL_SESSION_FILE_PARSE_CONCURRENCY,
  paginateItems,
  pickLiveSessionFiles,
  readAppendedJsonLines,
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
  AgentCallNode,
  AggregatedAiMessage,
  AiMessageAttachment,
  AiMessageFile,
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
  const parseConcurrency = context.sshHost ? REMOTE_SESSION_FILE_PARSE_CONCURRENCY : LOCAL_SESSION_FILE_PARSE_CONCURRENCY;

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
    const detail = await parseCodexSessionDetail(filePath, context, sessionId, cursor, limit, !!sourceRef);
    if (detail) return detail;
  }

  return null;
}

/** 화면의 전체 ID와 작업 범위가 모두 일치하는 메인 대화만 찾는다. 같은 ID의 복제 기록도 임의로 고르지 않는다 */
export async function findCodexMainThreadRollout(context: AiSessionReaderContext, threadId: string): Promise<string | null> {
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(threadId)) return null;
  const root = await getCodexSessionsDirectory(context);
  const files = (await listFilesRecursivelyBySuffix(root, ".jsonl", context.sshHost)).filter((file) => path.basename(file).endsWith(`-${threadId}.jsonl`));
  const matches: string[] = [];
  for (const file of files) {
    const meta = await readCodexSessionMeta(file, context.sshHost);
    if (meta?.threadId === threadId && !meta.parentThreadId && !meta.isSubagent && determineMatchScope(meta.cwd, context)) matches.push(file);
  }
  return matches.length === 1 ? matches[0] : null;
}

async function parseCodexSessionSummary(filePath: string, context: AiSessionReaderContext): Promise<AggregatedAiSession | null> {
  const meta = await readCodexSessionMeta(filePath, context.sshHost);
  if (!meta || !determineMatchScope(meta.cwd, context)) return null;

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
  const toolNames = new Map<string, string>();

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

    const parsedMessages = extractCodexEventMessages(event, filePath, toolNames);
    for (const message of parsedMessages) {
      if (shouldSkipCodexMessage(message.role, message.text)) {
        continue;
      }
      const messageKey = createCodexMessageKey(message.role, event.timestamp, message.text, message.attachments);
      if (seenMessages.has(messageKey)) {
        continue;
      }
      seenMessages.add(messageKey);

      if (message.role === "user" && !firstUserPrompt) {
        firstUserPrompt = message.text || message.attachments?.map(attachment => attachment.name).join(", ") || null;
      }

      if (matchesCodexQuery(context.query, message.text)) {
        hasQueryMatch = true;
      }

      messageCount += 1;
      if (message.role === "user" || message.role === "assistant") {
        updatedAt = toIsoString(event.timestamp) ?? updatedAt;
      }
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
  limit = DEFAULT_DETAIL_LIMIT,
  /** 호출자가 이 대화의 파일을 짚어 준 경우. 그 파일은 되풀이해 읽히므로 붙은 줄만 이어 읽는다 */
  isKnownSessionFile = false,
): Promise<AiSessionDetailReaderResult | null> {
  const readEvents = () => isKnownSessionFile && !context.sshHost ? readAppendedJsonLines(filePath) : readJsonLines(filePath, context.sshHost);
  const events = await getCachedOrParse(filePath, readEvents, context.sshHost);
  const transcript = readCodexSessionTranscript(events, filePath, sessionId, context);
  if (transcript.isOtherSession || !transcript.matchedPath) return null;
  transcript.newestFirstMessages ??= sortMessagesDescending(transcript.messages);
  const paginated = paginateItems(transcript.newestFirstMessages, cursor, limit);
  return createSessionDetail({
    sessionId,
    provider: "codex",
    title: transcript.title,
    matchedPath: transcript.matchedPath,
    sourceRef: filePath,
    messages: paginated.items,
    nextCursor: paginated.nextCursor,
  });
}

/** 한 rollout 파일의 대화가 남긴 메시지와, 그 파일의 이벤트를 어디까지 읽어 만들었는지 */
interface CodexSessionTranscript {
  readEventCount: number;
  /** 파일의 session_meta가 다른 대화나 다른 작업 경로를 가리킨다. 한 번 그렇게 판정되면 뒤에 붙는 줄도 읽지 않는다 */
  isOtherSession: boolean;
  matchedPath: string | null;
  title: string | null;
  /** 파일에 적힌 순서 */
  messages: AggregatedAiMessage[];
  /** messages를 최신순으로 정렬해 둔 것. 메시지가 늘면 버리고 다음에 다시 만든다 */
  newestFirstMessages: AggregatedAiMessage[] | null;
  seenMessages: Set<string>;
  toolNames: Map<string, string>;
}

/** 검색어처럼 조건이 계속 바뀌는 조회가 한 파일에 결과를 끝없이 쌓지 않도록 조건 수를 제한한다 */
const MAX_TRANSCRIPTS_PER_EVENT_LIST = 8;
/** 이벤트 배열이 살아 있는 동안만 그 배열로 만든 메시지를 기억한다. 파일을 처음부터 다시 읽으면 배열이 바뀌어 함께 버려진다 */
const codexSessionTranscripts = new WeakMap<unknown[], Map<string, CodexSessionTranscript>>();

/**
 * 이벤트 배열에서 한 대화의 메시지를 만든다. 같은 배열을 같은 조건으로 다시 받으면 지난번에 읽은 곳 뒤로 붙은 이벤트만 읽어,
 * 긴 대화를 보는 동안 매번 전체를 다시 만들지 않는다.
 */
function readCodexSessionTranscript(events: unknown[], filePath: string, sessionId: string, context: AiSessionReaderContext): CodexSessionTranscript {
  const transcripts = codexSessionTranscripts.get(events) ?? new Map<string, CodexSessionTranscript>();
  codexSessionTranscripts.set(events, transcripts);
  const conditions = JSON.stringify([filePath, sessionId, context]);
  const transcript = transcripts.get(conditions) ?? { readEventCount: 0, isOtherSession: false, matchedPath: null, title: null, messages: [], newestFirstMessages: null, seenMessages: new Set<string>(), toolNames: new Map<string, string>() };
  if (!transcripts.has(conditions) && transcripts.size >= MAX_TRANSCRIPTS_PER_EVENT_LIST) transcripts.delete(transcripts.keys().next().value!);
  transcripts.set(conditions, transcript);

  for (; !transcript.isOtherSession && transcript.readEventCount < events.length; transcript.readEventCount += 1) {
    const event = events[transcript.readEventCount] as CodexRolloutEvent;
    if (event.type === "session_meta") {
      const payload = event.payload ?? {};
      const candidateSessionId = typeof payload.id === "string" ? payload.id : path.basename(filePath, ".jsonl");
      const cwd = typeof payload.cwd === "string" ? payload.cwd : null;
      if (candidateSessionId !== sessionId || !determineMatchScope(cwd, context)) {
        transcript.isOtherSession = true;
        continue;
      }
      transcript.matchedPath = cwd;
      continue;
    }

    for (const message of extractCodexEventMessages(event, filePath, transcript.toolNames)) {
      if (!context.includeInstructions && shouldSkipCodexMessage(message.role, message.text)) {
        continue;
      }
      const messageKey = createCodexMessageKey(message.role, event.timestamp, message.text, message.attachments);
      if (transcript.seenMessages.has(messageKey)) {
        continue;
      }
      transcript.seenMessages.add(messageKey);

      if (context.roles && context.roles.length > 0 && !context.roles.includes(message.role)) {
        continue;
      }

      if (context.query && ![message.text, ...(message.attachments ?? []).map(attachment => attachment.name)].join("\n").toLowerCase().includes(context.query.toLowerCase())) {
        continue;
      }

      if (message.role === "user" && !transcript.title) {
        transcript.title = truncateText(message.text, 80);
      }

      const previewMessage = makePreviewMessage(message.role, event.timestamp, message.text, message.attachments);
      if (previewMessage) {
        transcript.messages.push(withToolCall(previewMessage, message));
        transcript.newestFirstMessages = null;
      }
    }
  }
  return transcript;
}

interface CodexEventMessage {
  role: AiMessageRole;
  text: string;
  attachments?: AiMessageAttachment[];
  toolName?: string;
  files?: AiMessageFile[];
}

function extractCodexEventMessages(event: CodexRolloutEvent, sourceRef = "", toolNames = new Map<string, string>()): CodexEventMessage[] {
  if (event.type === "response_item") {
    const payload = event.payload ?? {};
    if (typeof payload.call_id === "string" && typeof payload.name === "string" && ["function_call", "tool_call", "custom_tool_call"].includes(String(payload.type))) toolNames.set(payload.call_id, payload.name);
    return extractCodexPayloadMessages(payload, sourceRef).map((message) => message.role === "tool" && !message.toolName && typeof payload.call_id === "string" && toolNames.has(payload.call_id)
      ? { ...message, toolName: toolNames.get(payload.call_id) } : message);
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

function createCodexMessageKey(role: AiMessageRole, timestamp: string | undefined, text: string, attachments?: AiMessageAttachment[]): string {
  return `${role}\u0000${timestamp ?? ""}\u0000${text}\u0000${attachments?.map(attachment => attachment.id).join(",") ?? ""}`;
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

function extractCodexPayloadMessages(payload: Record<string, unknown>, sourceRef: string): CodexEventMessage[] {
  if (payload.type === "message") {
    const role = resolveCodexMessageRole(payload.role);
    const attachments = extractMessageAttachments(payload.content, sourceRef);
    const content = attachments.length && Array.isArray(payload.content)
      ? payload.content.filter(part => !part || typeof part !== "object" || typeof part.text !== "string" || !/^<image(?:\s[^>]*)?>$|^<\/image>$/.test(part.text.trim()))
      : payload.content;
    const rawText = extractPlainText(content);
    const text = attachments.length ? rawText.replace(/^(?:\[Image #\d+\]\s*)+/, "") : rawText;
    return text || attachments.length ? [{ role, text, ...(attachments.length ? { attachments } : {}) }] : [];
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
    return [{ role: "tool", text, toolName: name }];
  }

  if (payload.type === "custom_tool_call") {
    const toolCall = summarizeCodexCustomToolCall(payload);
    return [{ role: "tool", text: toolCall.summary, toolName: toolCall.toolName, files: toolCall.files }];
  }

  if (payload.type === "function_call_output" || payload.type === "custom_tool_call_output" || payload.type === "tool_result") {
    const text = extractPlainText(payload.output ?? payload.result ?? payload.content ?? payload);
    return text ? [{ role: "tool", text }] : [];
  }

  const text = extractPlainText(payload);
  return text ? [{ role: "unknown", text }] : [];
}

/**
 * 이 worktree에서 지금 돌고 있는 Codex 스레드와, 각 스레드가 띄운 서브에이전트를 찾는다.
 *
 * Codex는 서브에이전트도 자기 rollout 파일을 따로 만들고 첫 줄 `session_meta`에 부모 스레드를 남긴다.
 * 그래서 최근에 수정된 rollout만 골라 첫 줄만 읽으면 부모·자식 관계를 전부 복원할 수 있다.
 */
export async function readCodexLiveSessions(
  context: AiSessionReaderContext,
  windows: LiveAiSessionWindows,
): Promise<LiveProviderSnapshot[]> {
  const recentFiles = await listFilesModifiedWithin(
    await getCodexSessionsDirectory(context),
    ".jsonl",
    windows.recentWindowMs,
    context.sshHost,
  );

  const metaByFile = await mapWithConcurrency(
    recentFiles,
    context.sshHost ? REMOTE_SESSION_FILE_PARSE_CONCURRENCY : LOCAL_SESSION_FILE_PARSE_CONCURRENCY,
    async (file) => ({ file, meta: await readCodexSessionMeta(file.filePath, context.sshHost) }),
  );

  const threads = metaByFile.filter((entry): entry is { file: typeof entry.file; meta: CodexThreadMeta } =>
    entry.meta !== null);

  const ownThreads = pickLiveSessionFiles(
    threads
      .filter((thread) => !thread.meta.parentThreadId && determineMatchScope(thread.meta.cwd, context))
      .map((thread) => ({ ...thread.file, meta: thread.meta })),
    windows.runningWindowMs,
  );

  const runningSince = Date.now() - windows.runningWindowMs;

  return Promise.all(ownThreads.map(async (ownThread) => ({
    sessionId: ownThread.meta.threadId,
    currentTask: await readCodexCurrentActivity(ownThread.filePath, context.sshHost),
    lastActiveAt: new Date(ownThread.mtimeMs).toISOString(),
    runningSubtasks: threads
      .filter((thread) => thread.meta.parentThreadId === ownThread.meta.threadId)
      .filter((thread) => thread.file.mtimeMs >= runningSince)
      .map((thread) => ({
        id: thread.meta.threadId,
        name: thread.meta.agentNickname,
        lastActiveAt: new Date(thread.file.mtimeMs).toISOString(),
      })),
  })));
}

/**
 * 세션이 지금 무엇을 하는지는 마지막 AI 응답이 가장 잘 말해준다.
 * 아직 응답이 없는 갓 시작한 세션만 마지막 사용자 입력으로 되돌린다.
 */
async function readCodexCurrentActivity(
  filePath: string,
  sshHost: string | null | undefined,
): Promise<string | null> {
  try {
    const events = await readJsonLinesTail(filePath, CODEX_TAIL_EVENT_COUNT, sshHost);
    return findLatestCodexRoleText(events, "assistant") ?? findLatestCodexRoleText(events, "user");
  } catch {
    // rollout을 읽지 못해도 실행중 여부는 파일 활동만으로 판정할 수 있다.
    return null;
  }
}

function findLatestCodexRoleText(events: unknown[], role: "assistant" | "user"): string | null {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const payload = (events[index] as CodexRolloutEvent)?.payload;
    if (payload?.role !== role) continue;

    const text = extractPlainText(payload.content);
    if (text) {
      return truncateText(text, CODEX_CURRENT_TASK_LENGTH);
    }
  }

  return null;
}

const CODEX_TAIL_EVENT_COUNT = 60;
const CODEX_CURRENT_TASK_LENGTH = 80;

interface CodexThreadMeta {
  threadId: string;
  parentThreadId: string | null;
  isSubagent: boolean;
  agentNickname: string | null;
  cwd: string | null;
  startedAt: string | null;
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
      isSubagent: !!meta.source && typeof meta.source === "object" && "subagent" in meta.source,
      agentNickname: typeof meta.agent_nickname === "string" ? meta.agent_nickname : null,
      cwd: typeof meta.cwd === "string" ? meta.cwd : null,
      startedAt: toIsoString(meta.timestamp),
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

/**
 * 스레드 하나가 띄운 Codex 서브에이전트 호출 그래프를 만든다.
 *
 * 서브에이전트도 자기 rollout을 만들고 첫 줄 `session_meta.parent_thread_id`로 부모를 가리키므로,
 * 그 포인터를 계속 타고 내려가면 깊이 제한 없이 그래프가 복원된다.
 * 다만 끝났다는 기록은 남기지 않아, 한동안 움직이지 않은 스레드를 끝난 것으로 본다.
 */
export async function readCodexAgentCallGraph(
  context: AiSessionReaderContext,
  sessionId: string,
  windows: LiveAiSessionWindows,
): Promise<AgentCallNode[]> {
  const recentFiles = await listFilesModifiedWithin(
    await getCodexSessionsDirectory(context),
    ".jsonl",
    windows.recentWindowMs,
    context.sshHost,
  );

  const metaByFile = await mapWithConcurrency(
    recentFiles,
    context.sshHost ? REMOTE_SESSION_FILE_PARSE_CONCURRENCY : LOCAL_SESSION_FILE_PARSE_CONCURRENCY,
    async (file) => ({ file, meta: await readCodexSessionMeta(file.filePath, context.sshHost) }),
  );

  const threadsByParentId = new Map<string, CodexChildThread[]>();
  for (const { file, meta } of metaByFile) {
    if (!meta?.parentThreadId) {
      continue;
    }

    const siblings = threadsByParentId.get(meta.parentThreadId) ?? [];
    siblings.push({ meta, mtimeMs: file.mtimeMs });
    threadsByParentId.set(meta.parentThreadId, siblings);
  }

  return toCodexAgentCallNodes(sessionId, {
    threadsByParentId,
    runningSince: Date.now() - windows.runningWindowMs,
    visitedThreadIds: new Set<string>(),
  });
}

interface CodexChildThread {
  meta: CodexThreadMeta;
  mtimeMs: number;
}

interface CodexGraphIndex {
  threadsByParentId: Map<string, CodexChildThread[]>;
  runningSince: number;
  /** 기록이 서로를 부모로 가리키는 이상한 경우에도 재귀가 멈추도록 이미 그린 스레드를 기억한다 */
  visitedThreadIds: Set<string>;
}

function toCodexAgentCallNodes(parentThreadId: string, index: CodexGraphIndex): AgentCallNode[] {
  const childThreads = (index.threadsByParentId.get(parentThreadId) ?? [])
    .filter((child) => !index.visitedThreadIds.has(child.meta.threadId));

  return childThreads.map((child) => {
    index.visitedThreadIds.add(child.meta.threadId);

    return {
      id: child.meta.threadId,
      agentType: null,
      skill: null,
      task: child.meta.agentNickname,
      startedAt: child.meta.startedAt,
      endedAt: child.mtimeMs >= index.runningSince ? null : new Date(child.mtimeMs).toISOString(),
      children: toCodexAgentCallNodes(child.meta.threadId, index),
    };
  });
}
