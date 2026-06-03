import path from "path";
import {
  createReaderResult,
  createSessionDetail,
  determineMatchScope,
  extractPlainText,
  makePreviewMessage,
  mapWithConcurrency,
  paginateItems,
  REMOTE_SESSION_FILE_PARSE_CONCURRENCY,
  safeJsonParse,
  sortMessagesDescending,
  toIsoString,
  truncateText,
} from "@/lib/aiSessions/shared";
import { getHomeDirectory, listFilesRecursivelyBySuffix, pathExists, readTextFile } from "@/lib/hostFileAccess";
import type {
  AggregatedAiMessage,
  AggregatedAiSession,
  AiMessageRole,
  AiSessionDetailReaderResult,
  AiSessionReaderContext,
  AiSessionReaderResult,
} from "@/lib/aiSessions/types";

const DEFAULT_DETAIL_LIMIT = 20;

interface GeminiChatFile {
  id?: string;
  sessionId?: string;
  title?: string;
  projectPath?: string;
  cwd?: string;
  directory?: string;
  startTime?: string;
  createdAt?: string;
  timestamp?: string;
  lastUpdated?: string;
  updatedAt?: string;
  messages?: unknown[];
}

interface GeminiParsedChat {
  value: GeminiChatFile;
  projectId: string | null;
  matchedPath: string | null;
  matchScope: AggregatedAiSession["matchScope"] | null;
}

export async function readGeminiSessions(context: AiSessionReaderContext): Promise<AiSessionReaderResult> {
  const geminiRoot = await getGeminiRootDirectory(context);
  const rootExists = await pathExists(geminiRoot, context.sshHost);
  if (!rootExists) {
    return createReaderResult("gemini", { available: false, reason: "Gemini CLI directory not found" });
  }

  const tmpDirectory = path.join(geminiRoot, "tmp");
  const tmpExists = await pathExists(tmpDirectory, context.sshHost);
  if (!tmpExists) {
    return createReaderResult("gemini", { sessions: [], reason: "No local Gemini session files found" });
  }

  const projectPathById = await readGeminiProjectPathMap(geminiRoot, context);
  const chatFiles = (await listFilesRecursivelyBySuffix(tmpDirectory, ".json", context.sshHost))
    .filter((filePath) => path.basename(path.dirname(filePath)) === "chats");
  const parseConcurrency = context.sshHost ? REMOTE_SESSION_FILE_PARSE_CONCURRENCY : chatFiles.length || 1;
  const parsed = await mapWithConcurrency(
    chatFiles,
    parseConcurrency,
    (filePath) => parseGeminiSessionSummary(filePath, context, projectPathById),
  );
  const sessions = parsed.filter((session): session is AggregatedAiSession => session !== null);

  return createReaderResult("gemini", {
    sessions,
    reason: sessions.length === 0 ? "No Gemini sessions matched this task" : null,
  });
}

export async function readGeminiSessionDetail(
  context: AiSessionReaderContext,
  sessionId: string,
  sourceRef?: string | null,
  cursor?: string | null,
  limit = DEFAULT_DETAIL_LIMIT,
): Promise<AiSessionDetailReaderResult | null> {
  const geminiRoot = await getGeminiRootDirectory(context);
  const projectPathById = await readGeminiProjectPathMap(geminiRoot, context);
  const chatFiles = sourceRef
    ? [sourceRef]
    : (await listFilesRecursivelyBySuffix(path.join(geminiRoot, "tmp"), ".json", context.sshHost))
      .filter((filePath) => path.basename(path.dirname(filePath)) === "chats");

  for (const filePath of chatFiles) {
    const parsed = await parseGeminiChatFile(filePath, context, projectPathById);
    if (!parsed) continue;

    const candidateSessionId = resolveGeminiSessionId(parsed.value, filePath);
    if (candidateSessionId !== sessionId) continue;

    const messages = flattenGeminiMessages(parsed.value.messages ?? [])
      .filter((message) => {
        if (context.roles && context.roles.length > 0 && !context.roles.includes(message.role)) return false;
        if (context.query && !message.fullText.toLowerCase().includes(context.query.toLowerCase())) return false;
        return true;
      });

    const paginated = paginateItems(sortMessagesDescending(messages), cursor, limit);
    const firstUserPrompt = messages.find((message) => message.role === "user")?.fullText ?? null;
    const title = parsed.value.title ?? (firstUserPrompt ? truncateText(firstUserPrompt, 80) : null);

    return createSessionDetail({
      sessionId,
      provider: "gemini",
      title,
      matchedPath: parsed.matchedPath,
      sourceRef: filePath,
      messages: paginated.items,
      nextCursor: paginated.nextCursor,
    });
  }

  return null;
}

async function parseGeminiSessionSummary(
  filePath: string,
  context: AiSessionReaderContext,
  projectPathById: Map<string, string>,
): Promise<AggregatedAiSession | null> {
  const parsed = await parseGeminiChatFile(filePath, context, projectPathById);
  if (!parsed?.matchedPath || !parsed.matchScope) return null;

  const messages = flattenGeminiMessages(parsed.value.messages ?? []);
  const firstUserPrompt = messages.find((message) => message.role === "user")?.fullText ?? null;
  const sessionId = resolveGeminiSessionId(parsed.value, filePath);
  const title = parsed.value.title ?? (firstUserPrompt ? truncateText(firstUserPrompt, 80) : null);

  if (context.query && !matchesGeminiQuery(
    context.query,
    title,
    firstUserPrompt,
    parsed.matchedPath,
    sessionId,
    ...messages.map((message) => message.fullText),
  )) return null;

  return {
    id: sessionId,
    provider: "gemini",
    startedAt: toIsoString(parsed.value.startTime ?? parsed.value.createdAt ?? parsed.value.timestamp),
    updatedAt: toIsoString(parsed.value.lastUpdated ?? parsed.value.updatedAt ?? parsed.value.timestamp),
    matchedPath: parsed.matchedPath,
    matchScope: parsed.matchScope,
    title,
    firstUserPrompt: firstUserPrompt ? truncateText(firstUserPrompt) : null,
    messageCount: messages.length,
    sourceRef: filePath,
  };
}

async function parseGeminiChatFile(
  filePath: string,
  context: AiSessionReaderContext,
  projectPathById: Map<string, string>,
): Promise<GeminiParsedChat | null> {
  const content = await readTextFile(filePath, context.sshHost);
  const value = safeJsonParse<GeminiChatFile>(content);
  if (!value || typeof value !== "object") return null;

  const projectId = getGeminiProjectIdFromChatPath(filePath);
  const matchedPath = await resolveGeminiMatchedPath(value, filePath, projectId, projectPathById, context);
  const matchScope = determineMatchScope(matchedPath, context);
  if (!matchedPath || !matchScope) return null;

  return { value, projectId, matchedPath, matchScope };
}

function matchesGeminiQuery(query: string | undefined, ...values: Array<string | null | undefined>): boolean {
  if (!query) return true;
  const normalizedQuery = query.toLowerCase();
  return values.some((value) => value?.toLowerCase().includes(normalizedQuery));
}

function flattenGeminiMessages(messages: unknown[]): AggregatedAiMessage[] {
  const flattened: AggregatedAiMessage[] = [];

  for (const rawMessage of messages) {
    if (!rawMessage || typeof rawMessage !== "object") continue;
    const message = rawMessage as Record<string, unknown>;
    const timestamp = message.timestamp ?? message.createdAt ?? message.time;

    for (const toolMessage of extractGeminiToolMessages(message, timestamp)) {
      flattened.push(toolMessage);
    }

    for (const thoughtMessage of extractGeminiThoughtMessages(message, timestamp)) {
      flattened.push(thoughtMessage);
    }

    const role = resolveGeminiRole(message.type ?? message.role);
    const text = extractPlainText(message.content ?? message.parts ?? message.text ?? message.message);
    const previewMessage = text ? makePreviewMessage(role, timestamp, text) : null;
    if (previewMessage) flattened.push(previewMessage);
  }

  return flattened;
}

function extractGeminiThoughtMessages(message: Record<string, unknown>, timestamp: unknown): AggregatedAiMessage[] {
  const thoughts = message.thoughts ?? message.thought ?? message.reasoning;
  if (!thoughts) return [];

  const values = Array.isArray(thoughts) ? thoughts : [thoughts];
  return values
    .map((thought) => makePreviewMessage("reasoning", timestamp, extractPlainText(thought)))
    .filter((value): value is AggregatedAiMessage => Boolean(value));
}

function extractGeminiToolMessages(message: Record<string, unknown>, timestamp: unknown): AggregatedAiMessage[] {
  const rawToolCalls = message.toolCalls ?? message.tool_calls ?? message.functionCalls ?? message.function_calls ?? message.tools;
  if (!rawToolCalls) return [];

  const toolCalls = Array.isArray(rawToolCalls) ? rawToolCalls : [rawToolCalls];
  return toolCalls
    .map((toolCall) => makePreviewMessage("tool", timestamp, formatGeminiToolCall(toolCall)))
    .filter((value): value is AggregatedAiMessage => Boolean(value));
}

function formatGeminiToolCall(toolCall: unknown): string {
  if (!toolCall || typeof toolCall !== "object") {
    return extractPlainText(toolCall);
  }

  const record = toolCall as Record<string, unknown>;
  const name = typeof record.name === "string"
    ? record.name
    : typeof record.tool === "string"
      ? record.tool
      : typeof record.functionName === "string"
        ? record.functionName
        : "tool";
  const args = stringifyUnknown(record.args ?? record.arguments ?? record.input);
  const result = extractPlainText(record.result ?? record.output ?? record.response ?? record.content);

  return [
    name,
    args ? `args: ${args}` : null,
    result ? `result: ${result}` : null,
  ].filter(Boolean).join(" | ");
}

function stringifyUnknown(value: unknown): string {
  if (value === null || value === undefined) return "";
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  try {
    return JSON.stringify(value);
  } catch {
    return "";
  }
}

function resolveGeminiRole(role: unknown): AiMessageRole {
  if (role === "user") return "user";
  if (role === "gemini" || role === "assistant" || role === "model") return "assistant";
  if (role === "system" || role === "info") return "system";
  if (role === "tool") return "tool";
  if (role === "reasoning" || role === "thought") return "reasoning";
  if (role === "developer") return "developer";
  return "unknown";
}

function resolveGeminiSessionId(value: GeminiChatFile, filePath: string): string {
  return value.sessionId ?? value.id ?? path.basename(filePath, ".json");
}

async function resolveGeminiMatchedPath(
  value: GeminiChatFile,
  filePath: string,
  projectId: string | null,
  projectPathById: Map<string, string>,
  context: AiSessionReaderContext,
): Promise<string | null> {
  const directPath = value.cwd ?? value.projectPath ?? value.directory;
  if (directPath) return directPath;

  if (projectId) {
    const mappedProjectPath = projectPathById.get(projectId);
    if (mappedProjectPath) return mappedProjectPath;

    const projectRoot = (await readTextFile(path.join(path.dirname(path.dirname(filePath)), ".project_root"), context.sshHost)).trim();
    if (projectRoot) {
      projectPathById.set(projectId, projectRoot);
      return projectRoot;
    }
  }

  return null;
}

function getGeminiProjectIdFromChatPath(filePath: string): string | null {
  const chatsDirectory = path.dirname(filePath);
  if (path.basename(chatsDirectory) !== "chats") return null;
  return path.basename(path.dirname(chatsDirectory));
}

async function readGeminiProjectPathMap(geminiRoot: string, context: AiSessionReaderContext): Promise<Map<string, string>> {
  const content = await readTextFile(path.join(geminiRoot, "projects.json"), context.sshHost);
  const parsed = safeJsonParse<Record<string, unknown>>(content);
  const map = new Map<string, string>();
  if (!parsed) return map;

  const projects = parsed.projects && typeof parsed.projects === "object"
    ? parsed.projects as Record<string, unknown>
    : parsed;

  for (const [projectPath, value] of Object.entries(projects)) {
    if (typeof value === "string") {
      map.set(value, projectPath);
      continue;
    }

    if (value && typeof value === "object") {
      const record = value as Record<string, unknown>;
      const id = record.id ?? record.hash ?? record.projectId ?? record.projectHash;
      if (typeof id === "string") {
        map.set(id, projectPath);
      }
    }
  }

  return map;
}

async function getGeminiRootDirectory(context: AiSessionReaderContext): Promise<string> {
  return path.join(await getHomeDirectory(context.sshHost), ".gemini");
}
