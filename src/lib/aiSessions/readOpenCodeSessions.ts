import { execFile } from "child_process";
import { homedir } from "os";
import path from "path";
import { promisify } from "util";
import { execGit } from "@/lib/gitOperations";
import { getHomeDirectory, pathExists, quoteShellArgument } from "@/lib/hostFileAccess";
import {
  createReaderResult,
  createSessionDetail,
  determineMatchScope,
  extractPlainText,
  makePreviewMessage,
  safeJsonParse,
  sortMessagesDescending,
  toIsoString,
  truncateText,
} from "@/lib/aiSessions/shared";
import type {
  AggregatedAiMessage,
  AiMessageRole,
  AiSessionDetailReaderResult,
  AiSessionReaderContext,
  AiSessionReaderResult,
  LiveAiSessionWindows,
  LiveAiSubtask,
  LiveProviderSnapshot,
} from "@/lib/aiSessions/types";

const execFileAsync = promisify(execFile);
const OPEN_CODE_QUERY_LIMIT = 120;
const DEFAULT_DETAIL_LIMIT = 20;
// Remote detail queries return message bodies, so they need the same bounded large-session allowance as JSONL history reads.
const OPEN_CODE_REMOTE_QUERY_MAX_BUFFER_BYTES = 64 * 1024 * 1024;
const SQLITE_QUERY_SCRIPT = `
import base64
import json
import sqlite3
import sys

payload = json.loads(base64.b64decode(sys.argv[1]).decode("utf-8"))
conn = sqlite3.connect(payload["dbPath"])
try:
    conn.row_factory = sqlite3.Row
    rows = conn.execute(payload["sql"], payload.get("parameters") or {}).fetchall()
    print(json.dumps([dict(row) for row in rows], ensure_ascii=False))
finally:
    conn.close()
`.trim();

interface OpenCodeSessionRow {
  id: string;
  directory: string;
  title: string | null;
  time_created: number | null;
  time_updated: number | null;
  last_message_at?: number | null;
  part_count?: number | null;
  first_user_part?: string | null;
  matching_part_count?: number | null;
}

interface OpenCodeChildSessionRow {
  id: string;
  title: string | null;
  time_updated: number | null;
}

interface OpenCodeDetailRow {
  session_id: string;
  directory: string;
  title: string | null;
  message_id: string;
  part_data: string;
  time_created: number;
  message_data: string;
  total_count: number;
}

export async function readOpenCodeSessions(context: AiSessionReaderContext): Promise<AiSessionReaderResult> {
  const normalizedQuery = context.query?.toLowerCase();
  const escapedLikeQuery = normalizedQuery ? escapeSqliteLikePattern(normalizedQuery) : null;
  let rows: OpenCodeSessionRow[] | null;
  try {
    rows = await queryOpenCodeRows<OpenCodeSessionRow>(context,
      `SELECT
        s.id,
        s.directory,
        s.title,
        s.time_created,
        s.time_updated,
        (
          SELECT MAX(p.time_created)
          FROM part p
          JOIN message m ON m.id = p.message_id
          WHERE p.session_id = s.id
            AND json_extract(m.data, '$.role') IN ('user', 'assistant')
            AND json_extract(p.data, '$.type') = 'text'
        ) as last_message_at,
        (SELECT COUNT(*) FROM part p WHERE p.session_id = s.id) as part_count,
        (
          SELECT p.data
          FROM part p
          JOIN message m ON m.id = p.message_id
          WHERE p.session_id = s.id
            AND json_extract(m.data, '$.role') = 'user'
            AND json_extract(p.data, '$.type') = 'text'
          ORDER BY p.time_created ASC
          LIMIT 1
        ) as first_user_part,
        ${escapedLikeQuery
          ? `(SELECT COUNT(*) FROM part p WHERE p.session_id = s.id AND lower(p.data) LIKE '%' || @query || '%' ESCAPE '\\')`
          : '0'} as matching_part_count
      FROM session s
      ORDER BY COALESCE(last_message_at, s.time_updated) DESC
      LIMIT ${OPEN_CODE_QUERY_LIMIT};`,
      escapedLikeQuery ? { query: escapedLikeQuery } : undefined
    );
  } catch (error) {
    return createReaderResult("opencode", {
      available: false,
      reason: error instanceof Error ? error.message : "Failed to query OpenCode database",
    });
  }

  if (!rows) {
    return createReaderResult("opencode", { available: false, reason: "OpenCode database not found" });
  }

  const sessions = rows
    .filter((row) => determineMatchScope(row.directory, context))
    .filter((row) => {
      if (!normalizedQuery) return true;
      const firstUserPrompt = extractOpenCodePartText(row.first_user_part ?? "");
      const title = row.title ?? (firstUserPrompt ? truncateText(firstUserPrompt, 80) : null);
      return matchesOpenCodeQuery(normalizedQuery, title, firstUserPrompt, row.directory, row.id)
        || (row.matching_part_count ?? 0) > 0;
    })
    .map((row) => {
      const firstUserPrompt = extractOpenCodePartText(row.first_user_part ?? "");

      return {
        id: row.id,
        provider: "opencode" as const,
        startedAt: toIsoString(row.time_created),
        updatedAt: toIsoString(row.last_message_at ?? row.time_updated),
        matchedPath: row.directory,
        matchScope: determineMatchScope(row.directory, context)!,
        title: row.title ?? (firstUserPrompt ? truncateText(firstUserPrompt, 80) : null),
        firstUserPrompt: firstUserPrompt ? truncateText(firstUserPrompt) : null,
        messageCount: row.part_count ?? 0,
        sourceRef: row.id,
      };
    });

  return createReaderResult("opencode", {
    sessions,
    reason: sessions.length === 0 ? "No OpenCode sessions matched this task" : null,
  });
}

/**
 * 이 worktree에서 마지막으로 움직인 OpenCode 세션과, 그 세션이 띄운 자식 세션을 찾는다.
 *
 * OpenCode는 Task 도구가 만든 서브에이전트를 자식 세션으로 저장하고 부모를 `parent_id`로 가리킨다.
 * 다만 그 컬럼은 버전에 따라 없을 수 있어, 조회가 실패하면 서브태스크 없이 세션만 돌려준다.
 */
export async function readOpenCodeLiveSession(
  context: AiSessionReaderContext,
  windows: LiveAiSessionWindows,
): Promise<LiveProviderSnapshot | null> {
  const rows = await queryOpenCodeRows<OpenCodeSessionRow>(context,
    `SELECT s.id, s.directory, s.title, s.time_created, s.time_updated
      FROM session s
      ORDER BY s.time_updated DESC
      LIMIT ${OPEN_CODE_QUERY_LIMIT};`);

  const latestSession = rows?.find((row) => determineMatchScope(row.directory, context));
  if (!latestSession) {
    return null;
  }

  return {
    sessionId: latestSession.id,
    currentTask: latestSession.title,
    lastActiveAt: toIsoString(latestSession.time_updated),
    runningSubtasks: await readOpenCodeRunningChildren(context, latestSession.id, windows),
  };
}

async function readOpenCodeRunningChildren(
  context: AiSessionReaderContext,
  parentSessionId: string,
  windows: LiveAiSessionWindows,
): Promise<LiveAiSubtask[]> {
  try {
    const rows = await queryOpenCodeRows<OpenCodeChildSessionRow>(context,
      `SELECT s.id, s.title, s.time_updated
        FROM session s
        WHERE s.parent_id = @parentSessionId
        ORDER BY s.time_updated DESC;`,
      { parentSessionId });

    const runningSince = Date.now() - windows.runningWindowMs;

    return (rows ?? [])
      .filter((row) => (row.time_updated ?? 0) >= runningSince)
      .map((row) => ({
        id: row.id,
        name: row.title,
        lastActiveAt: toIsoString(row.time_updated),
      }));
  } catch {
    return [];
  }
}

export async function readOpenCodeSessionDetail(
  context: AiSessionReaderContext,
  sessionId: string,
  _sourceRef?: string | null,
  cursor?: string | null,
  limit = DEFAULT_DETAIL_LIMIT
): Promise<AiSessionDetailReaderResult | null> {
  const sid = sessionId;

  // 전체 메시지를 가져와서 서버에서 필터링 후 페이징 (SQLite JSON 필터링이 복잡하므로)
  const allRows = await queryOpenCodeRows<OpenCodeDetailRow>(context,
    `SELECT
      s.id AS session_id,
      s.directory,
      s.title,
      p.message_id,
      p.data AS part_data,
      p.time_created,
      m.data AS message_data
    FROM session s
    JOIN part p ON p.session_id = s.id
    JOIN message m ON m.id = p.message_id
    WHERE s.id = @sessionId
    ORDER BY p.time_created ASC;`,
    { sessionId: sid }
  );

  if (!allRows || allRows.length === 0) return null;

  const firstRow = allRows[0];
  if (!determineMatchScope(firstRow.directory, context)) return null;

  const filteredMessages = allRows
    .map((row) => {
      const parsedMessage = safeJsonParse<Record<string, unknown>>(row.message_data);
      const role = resolveOpenCodeRole(typeof parsedMessage?.role === "string" ? parsedMessage.role : undefined, row.part_data);
      const text = extractOpenCodePartText(row.part_data);

      if (context.roles && context.roles.length > 0 && !context.roles.includes(role)) return null;
      if (context.query && !text.toLowerCase().includes(context.query.toLowerCase())) return null;

      return makePreviewMessage(role, row.time_created, text);
    })
    .filter((value): value is AggregatedAiMessage => Boolean(value));

  const sorted = sortMessagesDescending(filteredMessages);
  const offset = cursor ? Number.parseInt(cursor, 10) : 0;
  const safeOffset = Number.isNaN(offset) ? 0 : offset;
  const pageItems = sorted.slice(safeOffset, safeOffset + limit);
  const nextCursor = (safeOffset + pageItems.length < sorted.length) ? String(safeOffset + pageItems.length) : null;

  const firstUserPrompt = sorted.find((message) => message.role === "user")?.fullText ?? null;

  return createSessionDetail({
    sessionId,
    provider: "opencode",
    title: firstRow.title ?? (firstUserPrompt ? truncateText(firstUserPrompt, 80) : null),
    matchedPath: firstRow.directory,
    sourceRef: firstRow.session_id,
    messages: pageItems,
    nextCursor,
  });
}

async function queryOpenCodeRows<T>(
  context: AiSessionReaderContext,
  sql: string,
  parameters?: Record<string, unknown>,
): Promise<T[] | null> {
  const dbPath = await getOpenCodeDatabasePath(context);
  if (!context.sshHost) {
    let nativeSqliteError: unknown = null;
    try {
      const { getSqliteConnection, querySqlite } = await import("@/lib/sqliteConnectionPool");
      const db = getSqliteConnection(dbPath);
      if (db) {
        return querySqlite<T>(db, sql, parameters);
      }
    } catch (error) {
      nativeSqliteError = error;
    }

    if (!await pathExists(dbPath)) {
      return null;
    }

    try {
      return await querySqliteRowsWithPython<T>(dbPath, sql, parameters);
    } catch (error) {
      if (nativeSqliteError instanceof Error) {
        throw new Error(`${error instanceof Error ? error.message : "Failed to query OpenCode database with Python sqlite"}; native sqlite unavailable: ${nativeSqliteError.message}`);
      }
      throw error;
    }
  }

  if (!await pathExists(dbPath, context.sshHost)) {
    return null;
  }

  const output = await execGit(
    `python3 -c ${quoteShellArgument(SQLITE_QUERY_SCRIPT)} ${quoteShellArgument(encodeSqliteQueryPayload(dbPath, sql, parameters))}`,
    context.sshHost,
    { maxBufferBytes: OPEN_CODE_REMOTE_QUERY_MAX_BUFFER_BYTES },
  );
  return parseSqliteQueryRows<T>(output);
}

async function querySqliteRowsWithPython<T>(
  dbPath: string,
  sql: string,
  parameters?: Record<string, unknown>,
): Promise<T[]> {
  const { stdout } = await execFileAsync(
    "python3",
    ["-c", SQLITE_QUERY_SCRIPT, encodeSqliteQueryPayload(dbPath, sql, parameters)],
    { encoding: "utf-8", maxBuffer: 10 * 1024 * 1024 },
  );

  return parseSqliteQueryRows<T>(stdout);
}

function encodeSqliteQueryPayload(
  dbPath: string,
  sql: string,
  parameters?: Record<string, unknown>,
): string {
  return Buffer.from(JSON.stringify({
    dbPath,
    sql,
    parameters: parameters ?? {},
  }), "utf-8").toString("base64");
}

function parseSqliteQueryRows<T>(output: string): T[] {
  const parsedRows = safeJsonParse<unknown>(output.trim());
  if (!Array.isArray(parsedRows)) {
    throw new Error("Failed to parse OpenCode database query result");
  }

  return parsedRows as T[];
}

async function getOpenCodeDatabasePath(context: AiSessionReaderContext): Promise<string> {
  if (!context.sshHost) {
    return path.join(homedir(), ".local", "share", "opencode", "opencode.db");
  }

  return path.posix.join(await getHomeDirectory(context.sshHost), ".local", "share", "opencode", "opencode.db");
}

function matchesOpenCodeQuery(query: string | undefined, ...values: Array<string | null | undefined>): boolean {
  if (!query) return true;
  return values.some((value) => value?.toLowerCase().includes(query));
}

function extractOpenCodePartText(rawData: string): string {
  const parsed = safeJsonParse<Record<string, unknown>>(rawData);
  if (!parsed) return "";

  if (parsed.type === "text" || parsed.type === "reasoning") {
    return extractPlainText(parsed.text ?? parsed);
  }

  if (parsed.type === "tool") {
    const toolName = typeof parsed.tool === "string" ? parsed.tool : "tool";
    const output = extractPlainText(parsed.state);
    return output ? `${toolName}: ${output}` : `${toolName} executed`;
  }

  return "";
}

function resolveOpenCodeRole(role: string | undefined, rawPartData?: string): AiMessageRole {
  const parsedPart = rawPartData ? safeJsonParse<Record<string, unknown>>(rawPartData) : null;
  if (parsedPart?.type === "tool") return "tool";
  if (parsedPart?.type === "reasoning") return "reasoning";
  if (role === "user") return "user";
  if (role === "assistant") return "assistant";
  if (role === "system") return "system";
  if (role === "developer") return "developer";
  if (role === "tool") return "tool";
  return "unknown";
}

function escapeSqliteLikePattern(value: string): string {
  return value
    .replaceAll("\\", "\\\\")
    .replaceAll("%", "\\%")
    .replaceAll("_", "\\_");
}
