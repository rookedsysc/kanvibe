import { execFile } from "child_process";
import path from "path";
import { promisify } from "util";
import { execGit } from "@/lib/gitOperations";
import { getHomeDirectory, pathExists, quoteShellArgument, readDirectoryFilesBySuffix } from "@/lib/hostFileAccess";
import {
  createReaderResult, createSessionDetail, determineMatchScope, extractPlainText,
  makePreviewMessage, paginateItems, safeJsonParse, sortSessionsDescending, toIsoString, truncateText,
} from "@/lib/aiSessions/shared";
import type {
  AggregatedAiMessage, AggregatedAiSession, AiMessageRole, AiSessionDetailReaderResult,
  AiSessionProvider, AiSessionReaderContext, AiSessionReaderResult,
} from "@/lib/aiSessions/types";

const executeFile = promisify(execFile);
const DEFAULT_DETAIL_LIMIT = 20;
const SQLITE_READ_MAX_BYTES = 64 * 1024 * 1024;
const SQLITE_READ_TIMEOUT_MS = 30_000;
const SQLITE_PROVIDERS = ["opencode2", "zcode"] as const;
type SqliteProvider = typeof SQLITE_PROVIDERS[number];

// Python SQLite는 Electron/Node ABI와 무관하고 SSH 호스트에서도 같은 읽기 전용 계약을 사용한다.
const SQLITE_AGENT_READ_SCRIPT = String.raw`
import base64,json,sqlite3,sys
from pathlib import Path
request=json.loads(base64.b64decode(sys.argv[1]).decode('utf-8'))
db=sqlite3.connect(Path(request['dbPath']).as_uri()+'?mode=ro',uri=True,timeout=5)
db.row_factory=sqlite3.Row
try:
    provider=request['provider']
    session_table='session_v2' if provider=='opencode2' else 'session'
    def columns(table):
        return {row['name'] for row in db.execute('PRAGMA table_info('+table+')')}
    session_columns=columns(session_table)
    required={'id','directory','time_created','time_updated'}
    if not required.issubset(session_columns):
        print(json.dumps({'schemaAvailable':False,'sessions':[],'messages':[]}))
        sys.exit(0)
    if provider=='opencode2':
        if not {'session_id','type','data','time_created','seq'}.issubset(columns('session_message')):
            raise ValueError('OpenCode 2 transcript schema is unreadable')
        source='session_message m'
        role='m.type'
        content='m.data'
        visible='1=1'
        order='m.time_created ASC,m.seq ASC'
    else:
        if not {'id','session_id','data','time_created'}.issubset(columns('message')) or not {'id','message_id','data','time_created'}.issubset(columns('part')):
            raise ValueError('ZCode transcript schema is unreadable')
        source='message m JOIN part p ON p.message_id=m.id'
        role="json_extract(m.data,'$.role')"
        content='p.data'
        visible="COALESCE(json_extract(m.data,'$.semantics.transcriptVisibility'),'visible')!='hidden'"
        message_sequence='m.sequence IS NULL ASC,m.sequence ASC,' if 'sequence' in columns('message') else ''
        part_sequence='p.sequence IS NULL ASC,p.sequence ASC,' if 'sequence' in columns('part') else ''
        order=message_sequence+'m.time_created ASC,m.rowid ASC,'+part_sequence+'p.time_created ASC,p.id ASC'
    title='s.title' if 'title' in session_columns else 'NULL'
    scope="(s.directory=:worktree OR substr(s.directory,1,length(:prefix))=:prefix)"
    parameters={'worktree':request['worktree'],'prefix':request['worktree'].rstrip('/\\')+request['separator'],'query':request.get('query','').lower(),'sessionId':request.get('sessionId')}
    message_search='EXISTS(SELECT 1 FROM '+source+' WHERE m.session_id=s.id AND '+visible+' AND instr(lower('+content+'),:query)>0)'
    search='(:query=\'\' OR instr(lower(COALESCE('+title+',\'\')),:query)>0 OR instr(lower(s.id),:query)>0 OR instr(lower(s.directory),:query)>0 OR '+message_search+')'
    query='SELECT s.id,s.directory,'+title+' AS title,s.time_created,s.time_updated FROM '+session_table+' s WHERE '+scope
    if request.get('sessionId') is not None:
        query+=' AND s.id=:sessionId'
    else:
        query+=' AND '+search
    query+=' ORDER BY s.time_updated DESC,s.id ASC'
    sessions=[dict(row) for row in db.execute(query,parameters)]
    messages=[]
    message_query='SELECT '+role+' AS role,'+content+' AS data,m.time_created FROM '+source+' WHERE m.session_id=:sessionId AND '+visible
    for session in sessions:
        bindings={'sessionId':session['id']}
        if provider=='opencode2':
            count_query='SELECT COUNT(*) FROM session_message m WHERE m.session_id=:sessionId'
        else:
            count_query='SELECT COUNT(*) FROM message m WHERE m.session_id=:sessionId AND '+visible
        session['message_count']=db.execute(count_query,bindings).fetchone()[0]
        first=db.execute(message_query+' AND '+role+"='user' ORDER BY "+order+' LIMIT 1',bindings).fetchone()
        session['first_user_data']=first['data'] if first else None
        if request.get('sessionId') is not None:
            messages=[dict(row) for row in db.execute(message_query+' ORDER BY '+order,bindings)]
    print(json.dumps({'schemaAvailable':True,'sessions':sessions,'messages':messages},ensure_ascii=False))
finally:
    db.close()
`.trim();

interface SqliteSessionRow {
  id: string;
  directory: string;
  title: string | null;
  time_created: number;
  time_updated: number;
  message_count: number;
  first_user_data: string | null;
}
interface SqliteMessageRow { role: string; data: string; time_created: number }
interface SqliteReadResult { schemaAvailable: boolean; sessions: SqliteSessionRow[]; messages: SqliteMessageRow[] }

export async function readSqliteAgentSessions(context: AiSessionReaderContext): Promise<AiSessionReaderResult[]> {
  return Promise.all(SQLITE_PROVIDERS.map((provider) => readProviderSessions(context, provider)));
}

async function readProviderSessions(context: AiSessionReaderContext, provider: SqliteProvider): Promise<AiSessionReaderResult> {
  try {
    const databases = await discoverDatabases(context, provider);
    const sessions = new Map<string, AggregatedAiSession>();
    let available = false;
    const failures: string[] = [];
    for (const dbPath of databases) {
      try {
        const result = await readDatabase(context, provider, dbPath);
        available ||= result.schemaAvailable;
        for (const row of result.sessions) {
          const matchScope = determineMatchScope(row.directory, context);
          if (!matchScope || sessions.has(row.id)) continue;
          const prompt = decodeSqliteContent(provider, row.first_user_data).text;
          sessions.set(row.id, {
            id: row.id, provider, matchScope, matchedPath: row.directory,
            title: row.title ?? (prompt ? truncateText(prompt, 80) : null),
            firstUserPrompt: prompt ? truncateText(prompt) : null,
            startedAt: toIsoString(row.time_created), updatedAt: toIsoString(row.time_updated),
            messageCount: row.message_count, sourceRef: dbPath,
          });
        }
      } catch (error) {
        failures.push(error instanceof Error ? error.message : "Database read failed");
      }
    }
    return createReaderResult(provider, {
      available, sessions: sortSessionsDescending([...sessions.values()]),
      reason: failures.length ? failures.join("; ") : available ? null : "Agent History database or schema not found",
    });
  } catch (error) {
    return createReaderResult(provider, { available: false, reason: error instanceof Error ? error.message : "Agent History unavailable" });
  }
}

export async function readSqliteAgentSessionDetail(
  context: AiSessionReaderContext,
  provider: AiSessionProvider,
  sessionId: string,
  sourceRef?: string | null,
  cursor?: string | null,
  limit = DEFAULT_DETAIL_LIMIT,
): Promise<AiSessionDetailReaderResult | null> {
  if (provider !== "opencode2" && provider !== "zcode") return null;
  const databases = await discoverDatabases(context, provider);
  if (sourceRef && !databases.includes(sourceRef)) return null;
  for (const dbPath of sourceRef ? [sourceRef] : databases) {
    const result = await readDatabase(context, provider, dbPath, sessionId);
    const session = result.sessions.find((row) => row.id === sessionId && determineMatchScope(row.directory, context));
    if (!session) continue;
    const messages: AggregatedAiMessage[] = [];
    for (const row of result.messages) {
      const decoded = decodeSqliteContent(provider, row.data);
      const role = decoded.role ?? resolveRole(row.role);
      if (context.roles?.length && !context.roles.includes(role)) continue;
      if (context.query && !decoded.text.toLowerCase().includes(context.query.toLowerCase())) continue;
      const message = makePreviewMessage(role, row.time_created, decoded.text);
      if (message) messages.push(message);
    }
    // ZCode의 sequence는 가져온 대화의 timestamp보다 우선한다.
    const page = paginateItems(messages.reverse(), cursor, limit);
    const prompt = decodeSqliteContent(provider, session.first_user_data).text;
    return createSessionDetail({
      provider, sessionId, sourceRef: dbPath, matchedPath: session.directory,
      title: session.title ?? (prompt ? truncateText(prompt, 80) : null),
      messages: page.items, nextCursor: page.nextCursor,
    });
  }
  return null;
}

async function discoverDatabases(context: AiSessionReaderContext, provider: SqliteProvider): Promise<string[]> {
  const home = await getHomeDirectory(context.sshHost);
  const paths = context.sshHost ? path.posix : path;
  if (provider === "zcode") {
    const db = paths.join(home, ".zcode", "cli", "db", "db.sqlite");
    return await pathExists(db, context.sshHost) ? [db] : [];
  }
  const directory = paths.join(home, ".local", "share", "opencode");
  if (!await pathExists(directory, context.sshHost)) return [];
  const files = await readDirectoryFilesBySuffix(directory, ".db", context.sshHost);
  return files.filter((file) => /^opencode(?:-[A-Za-z0-9_.-]+)?\.db$/.test(paths.basename(file)))
    .sort((left, right) => Number(paths.basename(right) === "opencode.db") - Number(paths.basename(left) === "opencode.db") || left.localeCompare(right));
}

async function readDatabase(
  context: AiSessionReaderContext,
  provider: SqliteProvider,
  dbPath: string,
  sessionId?: string,
): Promise<SqliteReadResult> {
  if (!context.worktreePath) return { schemaAvailable: true, sessions: [], messages: [] };
  const worktree = context.sshHost ? path.posix.resolve(context.worktreePath) : path.resolve(context.worktreePath);
  const payload = Buffer.from(JSON.stringify({ dbPath, provider, worktree, separator: context.sshHost ? "/" : path.sep, query: sessionId ? "" : context.query ?? "", sessionId })).toString("base64");
  const output = context.sshHost
    ? await execGit(`python3 -c ${quoteShellArgument(SQLITE_AGENT_READ_SCRIPT)} ${quoteShellArgument(payload)}`, context.sshHost, { maxBufferBytes: SQLITE_READ_MAX_BYTES })
    : (await executeFile("python3", ["-c", SQLITE_AGENT_READ_SCRIPT, payload], { encoding: "utf-8", maxBuffer: SQLITE_READ_MAX_BYTES, timeout: SQLITE_READ_TIMEOUT_MS })).stdout;
  const value = safeJsonParse<SqliteReadResult>(output.trim());
  if (!value || !Array.isArray(value.sessions) || !Array.isArray(value.messages)) throw new Error("Invalid Agent History database response");
  return value;
}

function decodeSqliteContent(provider: SqliteProvider, raw: string | null): { text: string; role?: AiMessageRole } {
  const record = raw ? safeJsonParse<Record<string, unknown>>(raw) : null;
  if (!record) return { text: "" };
  if (provider === "opencode2") {
    const content = record.content ?? record.text;
    if (Array.isArray(content)) {
      return { text: content.map((part) => {
        if (part && typeof part === "object" && (part as Record<string, unknown>).type === "tool") {
          const tool = part as Record<string, unknown>;
          return `${typeof tool.name === "string" ? tool.name : "tool"}: ${extractPlainText(tool.state)}`;
        }
        return extractPlainText(part);
      }).filter(Boolean).join("\n") };
    }
    return { text: extractPlainText(content) };
  }
  if (record.type === "reasoning") return { text: extractPlainText(record.text), role: "reasoning" };
  if (record.type === "tool") {
    const state = record.state && typeof record.state === "object" ? record.state as Record<string, unknown> : {};
    return { text: `${typeof record.tool === "string" ? record.tool : "tool"}: ${extractPlainText(state.output ?? state.input ?? state)}`, role: "tool" };
  }
  return { text: record.type === "text" ? extractPlainText(record.text) : "" };
}

function resolveRole(role: string): AiMessageRole {
  return role === "user" || role === "assistant" || role === "system" || role === "developer" || role === "reasoning" || role === "tool" ? role : "unknown";
}
