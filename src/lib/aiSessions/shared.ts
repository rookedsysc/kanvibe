import { createReadStream } from "fs";
import { open, readdir, readFile } from "fs/promises";
import { createInterface } from "readline";
import path from "path";
import { execGit } from "@/lib/gitOperations";
import { getFileMtimeMs, quoteShellArgument, readTextFile } from "@/lib/hostFileAccess";
import type {
  AggregatedAiMessage,
  AggregatedAiSession,
  AiMessageRole,
  AiSessionMatchScope,
  AiSessionProvider,
  AiSessionReaderContext,
  AiSessionReaderResult,
  AiSessionSourceStatus,
  AggregatedAiSessionsResult,
  AggregatedAiSessionDetail,
} from "@/lib/aiSessions/types";

const MAX_PREVIEW_MESSAGES = 12;
const MAX_PREVIEW_TEXT_LENGTH = 240;
export const REMOTE_SESSION_FILE_PARSE_CONCURRENCY = 3;

export function createReaderResult(provider: AiSessionProvider, partial?: Partial<AiSessionReaderResult>): AiSessionReaderResult {
  return {
    provider,
    available: partial?.available ?? true,
    sessionCount: partial?.sessionCount ?? partial?.sessions?.length ?? 0,
    reason: partial?.reason ?? null,
    sessions: partial?.sessions ?? [],
  };
}

export function createAggregationResult(partial?: Partial<AggregatedAiSessionsResult>): AggregatedAiSessionsResult {
  return {
    isRemote: partial?.isRemote ?? false,
    targetPath: partial?.targetPath ?? null,
    repoPath: partial?.repoPath ?? null,
    sessions: partial?.sessions ?? [],
    sources: partial?.sources ?? [],
    nextCursor: partial?.nextCursor ?? null,
  };
}

export function toSourceStatus(result: AiSessionReaderResult): AiSessionSourceStatus {
  return {
    provider: result.provider,
    available: result.available,
    sessionCount: result.sessionCount,
    reason: result.reason,
  };
}

/** 수정 시각이 가장 늦은 파일 하나. 실행중 판정은 "마지막으로 움직인 세션"에서 출발한다 */
export function pickLatestFile<T extends { mtimeMs: number }>(files: T[]): T | null {
  return files.reduce<T | null>(
    (latest, file) => (latest === null || file.mtimeMs > latest.mtimeMs ? file : latest),
    null,
  );
}

export function sortSessionsDescending(sessions: AggregatedAiSession[]): AggregatedAiSession[] {
  return [...sessions].sort((left, right) => {
    const leftValue = Date.parse(left.updatedAt ?? left.startedAt ?? "");
    const rightValue = Date.parse(right.updatedAt ?? right.startedAt ?? "");

    if (Number.isNaN(leftValue) && Number.isNaN(rightValue)) return 0;
    if (Number.isNaN(leftValue)) return 1;
    if (Number.isNaN(rightValue)) return -1;
    return rightValue - leftValue;
  });
}

export function sortMessagesDescending(messages: AggregatedAiMessage[]): AggregatedAiMessage[] {
  return [...messages].sort((left, right) => {
    const leftValue = Date.parse(left.timestamp ?? "");
    const rightValue = Date.parse(right.timestamp ?? "");

    if (Number.isNaN(leftValue) && Number.isNaN(rightValue)) return 0;
    if (Number.isNaN(leftValue)) return 1;
    if (Number.isNaN(rightValue)) return -1;
    return rightValue - leftValue;
  });
}

export function limitPreviewMessages(messages: AggregatedAiMessage[]): AggregatedAiMessage[] {
  return messages.slice(0, MAX_PREVIEW_MESSAGES);
}

function isTransportNoiseLine(trimmedLine: string): boolean {
  return [
    /^\[(?:remote-ssh|터미널|terminal)\]/,
    /^sshHost:/,
    /^command:/,
    /^sshArgs:/,
    /^error:/,
    /^at\s+.*:\d+:\d+\)?$/,
  ].some((pattern) => pattern.test(trimmedLine));
}

export function preserveMessageText(value: string): string {
  return value
    .replace(/\[Pasted ~\d+ lines\]/g, "\n")
    .split(/\r?\n/)
    .filter((line) => !isTransportNoiseLine(line.trim()))
    .join("\n")
    .replace(/[ \t]+$/gm, "")
    .trim();
}

export function normalizeText(value: string): string {
  const sanitized = preserveMessageText(value)
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .join(" ");

  return sanitized.replace(/\s+/g, " ").trim();
}

export function truncateText(value: string, maxLength = MAX_PREVIEW_TEXT_LENGTH): string {
  const normalized = normalizeText(value);
  if (normalized.length <= maxLength) return normalized;
  return `${normalized.slice(0, maxLength - 1).trimEnd()}...`;
}

function createTruncatedText(value: string, maxLength = MAX_PREVIEW_TEXT_LENGTH): {
  fullText: string;
  previewText: string;
  isTruncated: boolean;
} {
  const fullText = preserveMessageText(value);
  const previewText = truncateText(fullText, maxLength);

  return {
    fullText,
    previewText,
    isTruncated: previewText.length < normalizeText(fullText).length,
  };
}

export function toIsoString(value: unknown): string | null {
  if (typeof value === "string") {
    const parsed = Date.parse(value);
    return Number.isNaN(parsed) ? value : new Date(parsed).toISOString();
  }

  if (typeof value === "number") {
    if (value > 1_000_000_000_000) {
      return new Date(value).toISOString();
    }

    if (value > 1_000_000_000) {
      return new Date(value * 1000).toISOString();
    }
  }

  return null;
}

export function determineMatchScope(candidatePath: string | null | undefined, context: AiSessionReaderContext): AiSessionMatchScope | null {
  if (!candidatePath) return null;

  const normalizedCandidate = path.resolve(candidatePath);
  const normalizedWorktree = context.worktreePath ? path.resolve(context.worktreePath) : null;

  if (normalizedWorktree && isPathMatch(normalizedCandidate, normalizedWorktree)) {
    return "worktree";
  }

  return null;
}

export function getCandidatePaths(context: AiSessionReaderContext): string[] {
  return context.worktreePath ? [context.worktreePath] : [];
}

function isPathMatch(candidatePath: string, targetPath: string): boolean {
  return candidatePath === targetPath || candidatePath.startsWith(`${targetPath}${path.sep}`);
}

export async function listFilesRecursively(rootPath: string, matcher: (filePath: string) => boolean): Promise<string[]> {
  const entries = await readdir(rootPath, { withFileTypes: true });
  const files = await Promise.all(entries.map(async (entry) => {
    const entryPath = path.join(rootPath, entry.name);
    if (entry.isDirectory()) {
      return listFilesRecursively(entryPath, matcher);
    }

    return matcher(entryPath) ? [entryPath] : [];
  }));

  return files.flat();
}

export async function mapWithConcurrency<T, R>(
  items: T[],
  concurrency: number,
  mapper: (item: T, index: number) => Promise<R>,
): Promise<R[]> {
  if (items.length === 0) {
    return [];
  }

  const workerCount = Math.min(Math.max(1, Math.floor(concurrency)), items.length);
  const results = new Array<R>(items.length);
  let nextIndex = 0;

  async function runWorker(): Promise<void> {
    while (nextIndex < items.length) {
      const currentIndex = nextIndex;
      nextIndex += 1;
      results[currentIndex] = await mapper(items[currentIndex], currentIndex);
    }
  }

  await Promise.all(Array.from({ length: workerCount }, () => runWorker()));
  return results;
}

export async function readJsonLines(filePath: string, sshHost?: string | null): Promise<unknown[]> {
  const content = sshHost ? await readTextFile(filePath, sshHost) : await readFile(filePath, "utf-8");
  return content
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => safeJsonParse(line))
    .filter((value) => value !== null);
}

export function safeJsonParse<T>(value: string): T | null {
  try {
    return JSON.parse(value) as T;
  } catch {
    return null;
  }
}

export function extractPlainText(value: unknown): string {
  if (typeof value === "string") {
    return preserveMessageText(value);
  }

  if (Array.isArray(value)) {
    return preserveMessageText(value.map(extractPlainText).filter(Boolean).join("\n"));
  }

  if (!value || typeof value !== "object") {
    return "";
  }

  const record = value as Record<string, unknown>;

  const directKeys = ["text", "input_text", "output_text", "display", "description", "subject", "output", "result"];
  for (const key of directKeys) {
    if (typeof record[key] === "string") {
      return preserveMessageText(record[key] as string);
    }
  }

  if (record.content) {
    const fromContent = extractPlainText(record.content);
    if (fromContent) return fromContent;
  }

  if (record.message) {
    const fromMessage = extractPlainText(record.message);
    if (fromMessage) return fromMessage;
  }

  if (record.title && typeof record.title === "string") {
    return preserveMessageText(record.title);
  }

  return "";
}

export function makePreviewMessage(role: AiMessageRole, timestamp: unknown, text: string): AggregatedAiMessage | null {
  const truncated = createTruncatedText(text);
  if (!truncated.fullText) return null;

  return {
    role,
    timestamp: toIsoString(timestamp),
    text: truncated.previewText,
    fullText: truncated.fullText,
    isTruncated: truncated.isTruncated,
  };
}

export function finalizeSession(session: AggregatedAiSession): AggregatedAiSession {
  return {
    ...session,
    firstUserPrompt: session.firstUserPrompt ? truncateText(session.firstUserPrompt) : null,
    title: session.title ? truncateText(session.title, 80) : null,
  };
}

export function createSessionDetail(partial?: Partial<AggregatedAiSessionDetail>): AggregatedAiSessionDetail {
  return {
    sessionId: partial?.sessionId ?? "",
    provider: partial?.provider ?? "claude",
    title: partial?.title ?? null,
    matchedPath: partial?.matchedPath ?? null,
    sourceRef: partial?.sourceRef ?? null,
    messages: partial?.messages ?? [],
    nextCursor: partial?.nextCursor ?? null,
  };
}

export function paginateItems<T>(items: T[], cursor: string | null | undefined, limit: number): {
  items: T[];
  nextCursor: string | null;
} {
  const offset = cursor ? Number.parseInt(cursor, 10) : 0;
  const safeOffset = Number.isNaN(offset) ? 0 : offset;
  const pageItems = items.slice(safeOffset, safeOffset + limit);
  const nextOffset = safeOffset + pageItems.length;

  return {
    items: pageItems,
    nextCursor: nextOffset < items.length ? String(nextOffset) : null,
  };
}

interface FileParseCache<T> {
  mtime: number;
  result: T;
}

const FILE_PARSE_CACHE_MAX = 200;

/** filePath → { mtime, result } 형태로 파싱 결과를 캐시한다. 최대 FILE_PARSE_CACHE_MAX개 항목을 유지하고 초과 시 가장 오래된 항목을 제거한다. */
const fileParseCache = new Map<string, FileParseCache<unknown>>();

/** 앞 N줄 파싱 결과 전용 캐시. 실제 파일 경로를 키로 사용하되 전체 캐시와 분리 관리한다. */
const headParseCache = new Map<string, FileParseCache<unknown>>();

function evictOldestIfNeeded(cache: Map<string, unknown>): void {
  if (cache.size >= FILE_PARSE_CACHE_MAX) {
    const oldestKey = cache.keys().next().value;
    if (oldestKey !== undefined) cache.delete(oldestKey);
  }
}

/**
 * 파일의 mtime을 확인해 캐시가 유효하면 캐시를 반환하고, 변경됐으면 parseFn을 실행해 결과를 캐시한다.
 * @param filePath 대상 파일 경로
 * @param parseFn 캐시 miss 시 실행할 파싱 함수
 */
export async function getCachedOrParse<T>(filePath: string, parseFn: () => Promise<T>, sshHost?: string | null): Promise<T> {
  const mtime = await getFileMtimeMs(filePath, sshHost);
  if (mtime === null) {
    return parseFn();
  }

  const cached = fileParseCache.get(filePath) as FileParseCache<T> | undefined;
  if (cached && cached.mtime === mtime) {
    return cached.result;
  }

  const result = await parseFn();
  evictOldestIfNeeded(fileParseCache);
  fileParseCache.set(filePath, { mtime, result });
  return result;
}

/**
 * 파일 앞 N줄 파싱 결과를 mtime 기반으로 캐시한다. getCachedOrParse와 달리 실제 파일 경로만 키로 사용해 stat() 오류를 방지한다.
 * @param filePath 대상 파일 경로 (가상 suffix 없이 실제 경로)
 * @param parseFn 캐시 miss 시 실행할 파싱 함수
 */
export async function getCachedOrParseHead<T>(filePath: string, parseFn: () => Promise<T>, sshHost?: string | null): Promise<T> {
  const mtime = await getFileMtimeMs(filePath, sshHost);
  if (mtime === null) {
    return parseFn();
  }

  const cached = headParseCache.get(filePath) as FileParseCache<T> | undefined;
  if (cached && cached.mtime === mtime) {
    return cached.result;
  }

  const result = await parseFn();
  evictOldestIfNeeded(headParseCache);
  headParseCache.set(filePath, { mtime, result });
  return result;
}

/**
 * JSONL 파일의 앞 maxLines줄만 읽어 파싱한 결과를 반환한다.
 * 대용량 파일에서 세션 메타데이터만 필요할 때 전체 로드를 피하기 위해 사용한다.
 * @param filePath JSONL 파일 경로
 * @param maxLines 읽을 최대 줄 수
 */
export function readJsonLinesHead(filePath: string, maxLines: number, sshHost?: string | null): Promise<unknown[]> {
  if (sshHost) {
    return readTextFile(filePath, sshHost).then((content) => content
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean)
      .slice(0, maxLines)
      .map((line) => safeJsonParse(line))
      .filter((value) => value !== null));
  }

  return new Promise((resolve, reject) => {
    const results: unknown[] = [];
    const stream = createReadStream(filePath, { encoding: "utf-8" });
    const rl = createInterface({ input: stream, crlfDelay: Infinity });

    rl.on("line", (line) => {
      const trimmed = line.trim();
      if (trimmed) {
        const parsed = safeJsonParse(trimmed);
        if (parsed !== null) results.push(parsed);
      }
      if (results.length >= maxLines) {
        rl.close();
        stream.destroy();
      }
    });

    rl.on("close", () => resolve(results));
    rl.on("error", reject);
    stream.on("error", reject);
  });
}

export async function readJsonLinesTail(filePath: string, maxLines: number, sshHost?: string | null): Promise<unknown[]> {
  const content = sshHost
    ? await execGit(`tail -n ${maxLines} ${quoteShellArgument(filePath)}`, sshHost)
    : await readLocalFileTail(filePath);

  return content
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .slice(-maxLines)
    .map((line) => safeJsonParse(line))
    .filter((value) => value !== null);
}

// Sixty recent JSONL events commonly include large tool payloads; 128 KiB bounds I/O while leaving room for that window.
const LOCAL_TAIL_READ_BYTES = 128 * 1024;

async function readLocalFileTail(filePath: string): Promise<string> {
  const handle = await open(filePath, "r");
  try {
    const { size } = await handle.stat();
    const readLength = Math.min(size, LOCAL_TAIL_READ_BYTES);
    const buffer = Buffer.alloc(readLength);
    await handle.read(buffer, 0, readLength, size - readLength);
    return buffer.toString("utf-8");
  } finally {
    await handle.close();
  }
}
