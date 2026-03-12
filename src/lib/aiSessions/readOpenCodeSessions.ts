import { access } from "fs/promises";
import { execFile } from "child_process";
import { promisify } from "util";
import { homedir } from "os";
import path from "path";
import {
  createReaderResult,
  createSessionDetail,
  determineMatchScope,
  extractPlainText,
  makePreviewMessage,
  safeJsonParse,
  toIsoString,
  truncateText,
} from "@/lib/aiSessions/shared";
import type { AggregatedAiMessage, AiSessionDetailReaderResult, AiSessionReaderContext, AiSessionReaderResult } from "@/lib/aiSessions/types";

const execFileAsync = promisify(execFile);
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

interface OpenCodeMessageRoleRow {
  id: string;
  data: string;
}

interface OpenCodePartRow {
  message_id: string;
  data: string;
  time_created: number;
}

interface OpenCodeCountRow {
  count: number;
}

export async function readOpenCodeSessions(context: AiSessionReaderContext): Promise<AiSessionReaderResult> {
  const dbExists = await access(OPENCODE_DB_PATH).then(() => true).catch(() => false);
  if (!dbExists) {
    return createReaderResult("opencode", { available: false, reason: "OpenCode database not found" });
  }

  let rows: OpenCodeSessionRow[];
  try {
    rows = await querySqliteJson<OpenCodeSessionRow>(
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
  const rows = await querySqliteJson<OpenCodeSessionRow>(
    `SELECT id, directory, title, time_created, time_updated FROM session WHERE id = '${escapeSql(sessionId)}' LIMIT 1;`
  );
  const row = rows[0];
  if (!row) return null;
  if (!determineMatchScope(row.directory, context)) return null;

  const offset = cursor ? Number.parseInt(cursor, 10) : 0;
  const safeOffset = Number.isNaN(offset) ? 0 : offset;

  const [messageRows, partRows, countRows] = await Promise.all([
    querySqliteJson<OpenCodeMessageRoleRow>(`SELECT id, data FROM message WHERE session_id = '${escapeSql(sessionId)}';`),
    querySqliteJson<OpenCodePartRow>(
      `SELECT message_id, data, time_created FROM part WHERE session_id = '${escapeSql(sessionId)}' ORDER BY time_created ASC LIMIT ${limit} OFFSET ${safeOffset};`
    ),
    querySqliteJson<OpenCodeCountRow>(`SELECT COUNT(*) as count FROM part WHERE session_id = '${escapeSql(sessionId)}';`),
  ]);

  const roleByMessageId = new Map<string, string>();
  for (const message of messageRows) {
    const parsed = safeJsonParse<Record<string, unknown>>(message.data);
    const role = typeof parsed?.role === "string" ? parsed.role : null;
    if (role) roleByMessageId.set(message.id, role);
  }

  const messages: AggregatedAiMessage[] = partRows
    .map((part) => {
      const role = resolveOpenCodeRole(roleByMessageId.get(part.message_id));
      const text = extractOpenCodePartText(part.data);
      return makePreviewMessage(role, part.time_created, text);
    })
    .filter((value): value is AggregatedAiMessage => Boolean(value));

  const totalCount = countRows[0]?.count ?? messages.length;
  const nextCursor = safeOffset + messages.length < totalCount ? String(safeOffset + messages.length) : null;
  const firstUserPrompt = messages.find((message) => message.role === "user")?.fullText ?? null;

  return createSessionDetail({
    sessionId,
    provider: "opencode",
    title: row.title ?? (firstUserPrompt ? truncateText(firstUserPrompt, 80) : null),
    matchedPath: row.directory,
    sourceRef: row.id,
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

async function querySqliteJson<T>(sql: string): Promise<T[]> {
  const { stdout } = await execFileAsync("sqlite3", ["-json", OPENCODE_DB_PATH, sql], { maxBuffer: 10 * 1024 * 1024 });
  if (!stdout.trim()) return [];
  return JSON.parse(stdout) as T[];
}

function escapeSql(value: string): string {
  return value.replaceAll("'", "''");
}
