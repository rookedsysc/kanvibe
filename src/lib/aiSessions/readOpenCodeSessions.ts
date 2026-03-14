import { homedir } from "os";
import path from "path";
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
import { getSqliteConnection, querySqlite } from "@/lib/sqliteConnectionPool";
import type { AggregatedAiMessage, AiSessionDetailReaderResult, AiSessionReaderContext, AiSessionReaderResult } from "@/lib/aiSessions/types";

const OPENCODE_DB_PATH = path.join(homedir(), ".local", "share", "opencode", "opencode.db");
const OPEN_CODE_QUERY_LIMIT = 120;
const DEFAULT_DETAIL_LIMIT = 20;

interface OpenCodeSessionRow {
  id: string;
  directory: string;
  title: string | null;
  time_created: number | null;
  time_updated: number | null;
  part_count?: number | null;
  first_user_part?: string | null;
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
  const db = getSqliteConnection(OPENCODE_DB_PATH);
  if (!db) {
    return createReaderResult("opencode", { available: false, reason: "OpenCode database not found" });
  }

  let rows: OpenCodeSessionRow[];
  try {
    rows = querySqlite<OpenCodeSessionRow>(db,
      `SELECT
        s.id,
        s.directory,
        s.title,
        s.time_created,
        s.time_updated,
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
        ) as first_user_part
      FROM session s
      ORDER BY s.time_updated DESC
      LIMIT ${OPEN_CODE_QUERY_LIMIT};`
    );
  } catch (error) {
    return createReaderResult("opencode", {
      available: false,
      reason: error instanceof Error ? error.message : "Failed to query OpenCode database",
    });
  }

  const sessions = rows
    .filter((row) => determineMatchScope(row.directory, context))
    .map((row) => {
      const firstUserPrompt = extractOpenCodePartText(row.first_user_part ?? "");

      return {
        id: row.id,
        provider: "opencode" as const,
        startedAt: toIsoString(row.time_created),
        updatedAt: toIsoString(row.time_updated),
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

export async function readOpenCodeSessionDetail(
  context: AiSessionReaderContext,
  sessionId: string,
  _sourceRef?: string | null,
  cursor?: string | null,
  limit = DEFAULT_DETAIL_LIMIT
): Promise<AiSessionDetailReaderResult | null> {
  const offset = cursor ? Number.parseInt(cursor, 10) : 0;
  const safeOffset = Number.isNaN(offset) ? 0 : offset;
  const sid = escapeSql(sessionId);

  const db = getSqliteConnection(OPENCODE_DB_PATH);
  if (!db) return null;

  const detailRows = querySqlite<OpenCodeDetailRow>(db,
    `SELECT
      s.id AS session_id,
      s.directory,
      s.title,
      p.message_id,
      p.data AS part_data,
      p.time_created,
      m.data AS message_data,
      (SELECT COUNT(*) FROM part WHERE session_id = '${sid}') AS total_count
    FROM session s
    JOIN part p ON p.session_id = s.id
    JOIN message m ON m.id = p.message_id
    WHERE s.id = '${sid}'
    ORDER BY p.time_created ASC
    LIMIT ${limit} OFFSET ${safeOffset};`
  );

  if (detailRows.length === 0) return null;

  const firstRow = detailRows[0];
  if (!determineMatchScope(firstRow.directory, context)) return null;

  const messages = sortMessagesDescending(detailRows
    .map((row) => {
      const parsedMessage = safeJsonParse<Record<string, unknown>>(row.message_data);
      const role = resolveOpenCodeRole(typeof parsedMessage?.role === "string" ? parsedMessage.role : undefined);
      const text = extractOpenCodePartText(row.part_data);
      return makePreviewMessage(role, row.time_created, text);
    })
    .filter((value): value is AggregatedAiMessage => Boolean(value)));

  const totalCount = firstRow.total_count;
  const nextCursor = safeOffset + messages.length < totalCount ? String(safeOffset + messages.length) : null;
  const firstUserPrompt = messages.find((message) => message.role === "user")?.fullText ?? null;

  return createSessionDetail({
    sessionId,
    provider: "opencode",
    title: firstRow.title ?? (firstUserPrompt ? truncateText(firstUserPrompt, 80) : null),
    matchedPath: firstRow.directory,
    sourceRef: firstRow.session_id,
    messages,
    nextCursor,
  });
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

function resolveOpenCodeRole(role: string | undefined): "user" | "assistant" | "unknown" {
  if (role === "user") return "user";
  if (role === "assistant") return "assistant";
  return "unknown";
}

function escapeSql(value: string): string {
  return value.replaceAll("'", "''");
}
