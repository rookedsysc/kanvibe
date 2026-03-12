import { readdir, readFile } from "fs/promises";
import path from "path";
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

export function limitPreviewMessages(messages: AggregatedAiMessage[]): AggregatedAiMessage[] {
  return messages.slice(0, MAX_PREVIEW_MESSAGES);
}

export function normalizeText(value: string): string {
  return value.replace(/\s+/g, " ").trim();
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
  const fullText = normalizeText(value);
  const previewText = truncateText(fullText, maxLength);

  return {
    fullText,
    previewText,
    isTruncated: previewText.length < fullText.length,
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
  const normalizedRepo = context.repoPath ? path.resolve(context.repoPath) : null;

  if (normalizedWorktree && isPathMatch(normalizedCandidate, normalizedWorktree)) {
    return "worktree";
  }

  if (context.includeRepoSessions && normalizedRepo && isPathMatch(normalizedCandidate, normalizedRepo)) {
    return "repo";
  }

  return null;
}

export function getCandidatePaths(context: AiSessionReaderContext): string[] {
  const candidates: string[] = [];

  if (context.worktreePath) {
    candidates.push(context.worktreePath);
  }

  if (
    context.includeRepoSessions &&
    context.repoPath &&
    context.repoPath !== context.worktreePath
  ) {
    candidates.push(context.repoPath);
  }

  return Array.from(new Set(candidates));
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

export async function readJsonLines(filePath: string): Promise<unknown[]> {
  const content = await readFile(filePath, "utf-8");
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
    return normalizeText(value);
  }

  if (Array.isArray(value)) {
    return normalizeText(value.map(extractPlainText).filter(Boolean).join(" "));
  }

  if (!value || typeof value !== "object") {
    return "";
  }

  const record = value as Record<string, unknown>;

  const directKeys = ["text", "input_text", "output_text", "display"];
  for (const key of directKeys) {
    if (typeof record[key] === "string") {
      return normalizeText(record[key] as string);
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
    return normalizeText(record.title);
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
