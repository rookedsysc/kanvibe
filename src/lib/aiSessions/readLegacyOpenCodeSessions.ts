import path from "path";
import { lstat, realpath } from "fs/promises";
import { getHomeDirectory, listFilesRecursivelyBySuffix, pathExists, readDirectoryFilesBySuffix, readTextFile } from "@/lib/hostFileAccess";
import { createReaderResult, createSessionDetail, determineMatchScope, extractPlainText, getCachedOrParse, makePreviewMessage, mapWithConcurrency, paginateItems, toIsoString, truncateText } from "@/lib/aiSessions/shared";
import type { AggregatedAiMessage, AggregatedAiSession, AiSessionDetailReaderResult, AiSessionReaderContext, AiSessionReaderResult } from "@/lib/aiSessions/types";

const FILE_CONCURRENCY = 4;
const DEFAULT_DETAIL_LIMIT = 20;
const SAFE_STORAGE_ID = /^[A-Za-z0-9_-]+$/;
type JsonRecord = Record<string, unknown>;
interface LegacySession {
  summary: AggregatedAiSession;
  messages: AggregatedAiMessage[];
}
function record(value: unknown): JsonRecord {
  return value !== null && typeof value === "object" && !Array.isArray(value) ? value as JsonRecord : {};
}
function string(value: unknown): string | null { return typeof value === "string" && value.trim() ? value : null; }
function within(file: string, root: string): boolean {
  const relative = path.relative(path.resolve(root), path.resolve(file));
  return relative !== "" && relative !== ".." && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative);
}
async function storageDirectory(context: AiSessionReaderContext): Promise<string> {
  const home = await getHomeDirectory(context.sshHost);
  const configured = !context.sshHost && process.env.XDG_DATA_HOME?.trim();
  return path.join(configured && path.isAbsolute(configured) ? configured : path.join(home, ".local", "share"), "opencode", "storage");
}
async function readJson(file: string, root: string, context: AiSessionReaderContext): Promise<JsonRecord | null> {
  if (!within(file, root) || !file.endsWith(".json")) return null;
  try {
    if (!context.sshHost && ((await lstat(file)).isSymbolicLink() || !within(await realpath(file), await realpath(root)))) return null;
    const raw = await getCachedOrParse(file, () => readTextFile(file, context.sshHost), context.sshHost);
    const value = JSON.parse(raw) as unknown;
    return value !== null && typeof value === "object" && !Array.isArray(value) ? value as JsonRecord : null;
  } catch {
    // 오래된 설치의 부분 쓰기·삭제 파일 하나가 나머지 대화를 가리지 않게 한다.
    return null;
  }
}
async function messageContent(message: JsonRecord, id: string, root: string, context: AiSessionReaderContext): Promise<string> {
  const inline = extractPlainText(message.content);
  if (inline) return inline;
  if (!SAFE_STORAGE_ID.test(id)) return "";
  const files = await readDirectoryFilesBySuffix(path.join(root, "part", id), ".json", context.sshHost);
  const parts = await mapWithConcurrency(files.sort(), FILE_CONCURRENCY, (file) => readJson(file, root, context));
  return parts.filter((part): part is JsonRecord => part !== null)
    .filter((part) => part.messageID === undefined || part.messageID === id)
    .map((part) => extractPlainText(part)).filter(Boolean).join("\n");
}
async function readLegacySession(file: string, root: string, context: AiSessionReaderContext): Promise<LegacySession | null> {
  if (!within(file, path.join(root, "session"))) return null;
  const metadata = await readJson(file, root, context);
  if (!metadata) return null;
  const id = string(metadata.id) ?? path.basename(file, ".json");
  const directory = string(metadata.directory);
  const matchScope = determineMatchScope(directory, context);
  if (!SAFE_STORAGE_ID.test(id) || !matchScope) return null;
  const messageFiles = await readDirectoryFilesBySuffix(path.join(root, "message", id), ".json", context.sshHost);
  const values = await mapWithConcurrency(messageFiles, FILE_CONCURRENCY, async (messageFile) => ({ file: messageFile, value: await readJson(messageFile, root, context) }));
  const ordered = values.filter((entry): entry is { file: string; value: JsonRecord } => entry.value !== null)
    .sort((left, right) => {
      const a = toIsoString(record(left.value.time).created);
      const b = toIsoString(record(right.value.time).created);
      return (a ? Date.parse(a) : Infinity) - (b ? Date.parse(b) : Infinity) || left.file.localeCompare(right.file);
    });
  const messages = (await mapWithConcurrency(ordered, FILE_CONCURRENCY, async ({ file: messageFile, value }) => {
    if (value.sessionID !== undefined && value.sessionID !== id) return null;
    const role = value.role;
    if (role !== "user" && role !== "assistant" && role !== "tool" && role !== "system") return null;
    const messageId = string(value.id) ?? path.basename(messageFile, ".json");
    const text = await messageContent(value, messageId, root, context);
    return makePreviewMessage(role, record(value.time).created, text);
  })).filter((message): message is AggregatedAiMessage => message !== null);
  const firstUserPrompt = messages.find((message) => message.role === "user")?.fullText ?? null;
  const title = string(metadata.title) ?? (firstUserPrompt ? truncateText(firstUserPrompt, 80) : null);
  const created = toIsoString(record(metadata.time).created);
  const updated = messages.map((message) => message.timestamp).filter((timestamp): timestamp is string => timestamp !== null).sort().at(-1) ?? toIsoString(record(metadata.time).updated);
  return { summary: { id, provider: "opencode", title, firstUserPrompt: firstUserPrompt ? truncateText(firstUserPrompt) : null, matchedPath: directory, matchScope, sourceRef: file, startedAt: created, updatedAt: updated, messageCount: messages.length }, messages };
}
export async function readLegacyOpenCodeSessions(context: AiSessionReaderContext): Promise<AiSessionReaderResult> {
  try {
    const root = await storageDirectory(context);
    const available = await pathExists(path.join(root, "session"), context.sshHost);
    if (!available) return createReaderResult("opencode", { available: false, reason: "Legacy OpenCode storage not found" });
    const files = await listFilesRecursivelyBySuffix(path.join(root, "session"), ".json", context.sshHost);
    const values = await mapWithConcurrency(files, FILE_CONCURRENCY, (file) => readLegacySession(file, root, context));
    const query = context.query?.toLowerCase();
    const sessions = values.filter((value): value is LegacySession => value !== null).filter((value) => !query || [value.summary.id, value.summary.title, value.summary.matchedPath, ...value.messages.map((message) => message.fullText)].some((text) => text?.toLowerCase().includes(query))).map((value) => value.summary);
    return createReaderResult("opencode", { sessions });
  } catch {
    // SQLite만 있는 설치나 읽을 수 없는 legacy 저장소는 기존 SQLite 조회를 막지 않는다.
    return createReaderResult("opencode", { available: false, reason: "Legacy OpenCode storage could not be read" });
  }
}
export async function readLegacyOpenCodeSessionDetail(
  context: AiSessionReaderContext, sessionId: string, sourceRef?: string | null,
  cursor?: string | null, limit = DEFAULT_DETAIL_LIMIT,
): Promise<AiSessionDetailReaderResult | null> {
  if (!SAFE_STORAGE_ID.test(sessionId)) return null;
  const root = await storageDirectory(context);
  if (sourceRef && !within(sourceRef, path.join(root, "session"))) return null;
  const files = sourceRef ? [sourceRef] : await listFilesRecursivelyBySuffix(path.join(root, "session"), ".json", context.sshHost);
  for (const file of files) {
    const parsed = await readLegacySession(file, root, context);
    if (!parsed || parsed.summary.id !== sessionId) continue;
    const filtered = parsed.messages.filter((message) => (!context.roles?.length || context.roles.includes(message.role)) && (!context.query || message.fullText.toLowerCase().includes(context.query.toLowerCase())));
    const page = paginateItems([...filtered].reverse(), cursor, limit);
    return createSessionDetail({ sessionId, provider: "opencode", title: parsed.summary.title, matchedPath: parsed.summary.matchedPath, sourceRef: file, messages: page.items, nextCursor: page.nextCursor });
  }
  return null;
}
