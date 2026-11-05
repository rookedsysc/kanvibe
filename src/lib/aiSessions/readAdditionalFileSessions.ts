import path from "path";
import { realpath, lstat } from "fs/promises";
import {
  createReaderResult, createSessionDetail, determineMatchScope, getCachedOrParse, getCachedOrParseHead, readJsonLinesHead,
  mapWithConcurrency, paginateItems, safeJsonParse, truncateText,
} from "@/lib/aiSessions/shared";
import { getHomeDirectory, listFilesRecursivelyBySuffix, pathExists, readTextFile } from "@/lib/hostFileAccess";
import { decodeAdditionalFileSession, sessionRecord, sessionString, type AdditionalFileSession } from "@/lib/aiSessions/additionalFileSessionFormats";
import { readDevinSessionIndex } from "@/lib/aiSessions/readDevinSessionIndex";
import type { AggregatedAiSession, AiSessionDetailReaderResult, AiSessionProvider, AiSessionReaderContext, AiSessionReaderResult } from "@/lib/aiSessions/types";

interface FileSource {
  provider: AiSessionProvider;
  roots: string[];
  suffix: ".json" | ".jsonl";
  accepts?: (file: string) => boolean;
}
interface ReadScope {
  home: string;
  context: AiSessionReaderContext;
  files: Map<string, Promise<unknown>>;
  devinIndexes: Map<string, ReturnType<typeof readDevinSessionIndex>>;
  cursorMetadataFiles?: Promise<string[]>;
}
// 프로바이더와 파일 작업 수를 각각 제한해 저장소 수가 늘어도 디스크·SSH 요청이 폭증하지 않게 한다.
const PROVIDER_CONCURRENCY = 3;
const FILE_CONCURRENCY = 4;
const DEFAULT_DETAIL_LIMIT = 20;
const SESSION_METADATA_HEAD_LINES = 1;
const HEADER_SCOPED_PROVIDERS = new Set<AiSessionProvider>(["qoder", "codebuddy", "pi", "omp", "prime-agent", "openclaw", "copilot", "droid", "muse"]);
const ANTIGRAVITY_HISTORY_MATCH_WINDOW_MS = 2_000;
const UUID_JSON = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\.json$/i;

function fileSources(home: string, remote: boolean): FileSource[] {
  const root = (...segments: string[]) => path.join(home, ...segments);
  const override = (name: string, fallback: string) => !remote && process.env[name]?.trim() && path.isAbsolute(process.env[name]!.trim()) ? process.env[name]!.trim() : fallback;
  const noSubagents = (file: string) => !file.split(path.sep).includes("subagents");
  const graphFile = (file: string) => !file.split(path.sep).some((part) => ["tasks", "artifacts", "subagents"].includes(part));
  return [
    { provider: "qoder", roots: [root(".qoder", "projects")], suffix: ".jsonl", accepts: noSubagents },
    { provider: "codebuddy", roots: [root(".codebuddy", "projects")], suffix: ".jsonl", accepts: noSubagents },
    { provider: "hermes", roots: [path.join(override("HERMES_HOME", root(".hermes")), "sessions")], suffix: ".json", accepts: (file) => path.basename(file).startsWith("session_") },
    { provider: "pi", roots: [normalizeAgentSessions(override("PI_CODING_AGENT_DIR", root(".pi", "agent", "sessions")))], suffix: ".jsonl", accepts: graphFile },
    { provider: "omp", roots: [normalizeAgentSessions(override("OMP_CODING_AGENT_DIR", root(".omp", "agent", "sessions"))), normalizeAgentSessions(override("PI_CODING_AGENT_DIR", root(".pi", "agent", "sessions")))], suffix: ".jsonl", accepts: graphFile },
    { provider: "prime-agent", roots: [override("PRIME_AGENT_SESSION_DIR", override("PRIME_AGENT_CODING_AGENT_SESSION_DIR", path.join(override("PRIME_AGENT_CODING_AGENT_DIR", root(".prime", "agent")), "sessions")))], suffix: ".jsonl" },
    { provider: "cursor", roots: [root(".cursor", "projects")], suffix: ".jsonl", accepts: (file) => file.split(path.sep).includes("agent-transcripts") },
    { provider: "antigravity", roots: [root(".gemini", "antigravity-cli", "brain")], suffix: ".jsonl", accepts: (file) => /[/\\]\.system_generated[/\\]logs[/\\]transcript(?:_full)?\.jsonl$/.test(file) },
    { provider: "rovo", roots: [root(".rovodev", "sessions")], suffix: ".json", accepts: (file) => path.basename(file) === "metadata.json" },
    { provider: "copilot", roots: [path.join(override("COPILOT_HOME", root(".copilot")), "session-state")], suffix: ".jsonl" },
    { provider: "grok", roots: [path.join(override("GROK_HOME", root(".grok")), "sessions")], suffix: ".json", accepts: (file) => path.basename(file) === "summary.json" },
    { provider: "openclaw", roots: [path.join(override("OPENCLAW_STATE_DIR", root(".openclaw")), "agents"), root(".clawdbot", "agents")], suffix: ".jsonl", accepts: (file) => file.split(path.sep).includes("sessions") },
    { provider: "devin", roots: [path.join(override("DEVIN_HOME", root(".local", "share", "devin", "cli")), "transcripts"), path.join(override("DEVIN_HOME", root(".local", "share", "devin", "cli")), "agent_logs")], suffix: ".json" },
    { provider: "droid", roots: [root(".factory", "sessions"), root(".factory", "projects")], suffix: ".jsonl" },
    { provider: "cline", roots: [override("CLINE_SESSION_DATA_DIR", root(".cline", "data", "sessions"))], suffix: ".json", accepts: (file) => path.basename(file) === `${path.basename(path.dirname(file))}.json` },
    { provider: "kimi", roots: [path.join(override("KIMI_CODE_HOME", root(".kimi-code")), "sessions")], suffix: ".json", accepts: (file) => path.basename(file) === "state.json" && path.basename(path.dirname(file)).startsWith("session_") },
    { provider: "muse", roots: [path.join(override("XDG_DATA_HOME", root(".local", "share")), "muse", "sessions")], suffix: ".jsonl", accepts: (file) => path.basename(file) === "session.jsonl" },
    { provider: "jcode", roots: [path.join(override("JCODE_HOME", root(".jcode")), "sessions")], suffix: ".json", accepts: (file) => path.basename(file).startsWith("session_") },
    { provider: "kiro", roots: [path.join(override("KIRO_HOME", root(".kiro")), "sessions", "cli")], suffix: ".json", accepts: (file) => UUID_JSON.test(path.basename(file)) },
  ];
}
function normalizeAgentSessions(directory: string): string {
  const leaf = path.basename(directory);
  if (leaf === ".pi" || leaf === ".omp") return path.join(directory, "agent", "sessions");
  return leaf === "agent" ? path.join(directory, "sessions") : directory;
}
function isWithin(file: string, root: string): boolean {
  const relative = path.relative(path.resolve(root), path.resolve(file));
  return relative !== "" && relative !== ".." && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative);
}
async function allowedSource(file: string, source: FileSource, context: AiSessionReaderContext): Promise<boolean> {
  if (!file.endsWith(source.suffix) || (source.accepts && !source.accepts(file))) return false;
  for (const root of source.roots) {
    if (!isWithin(file, root)) continue;
    if (context.sshHost) return true;
    try {
      if (isWithin(await realpath(file), await realpath(root))) return true;
    } catch {
      // 삭제 중인 파일은 다음 목록 조회에서 다시 발견하므로 상세 후보에서 제외한다.
    }
  }
  return false;
}
function createScope(home: string, context: AiSessionReaderContext): ReadScope {
  return { home, context, files: new Map(), devinIndexes: new Map() };
}
async function readRegularFile(scope: ReadScope, file: string): Promise<string> {
  if (!scope.context.sshHost && (await lstat(file)).isSymbolicLink()) throw new Error("Session file cannot be a symbolic link");
  return getCachedOrParse(file, () => readTextFile(file, scope.context.sshHost), scope.context.sshHost);
}
function readParsedFile(scope: ReadScope, file: string): Promise<unknown> {
  let pending = scope.files.get(file);
  if (!pending) {
    pending = readRegularFile(scope, file).then((content) => {
      if (!content.trim()) return null;
      return file.endsWith(".jsonl") ? content.split(/\r?\n/).flatMap((line) => {
        const parsed = safeJsonParse<unknown>(line);
        return parsed === null ? [] : [parsed];
      }) : JSON.parse(content) as unknown;
    });
    scope.files.set(file, pending);
  }
  return pending;
}
async function optionalFile(scope: ReadScope, file: string): Promise<unknown> {
  return await pathExists(file, scope.context.sshHost) ? readParsedFile(scope, file) : null;
}
async function discoverSource(source: FileSource, context: AiSessionReaderContext): Promise<string[]> {
  const files = (await Promise.all(source.roots.map((root) => listFilesRecursivelyBySuffix(root, source.suffix, context.sshHost)))).flat();
  const unique = [...new Set(files)].filter((file) => !source.accepts || source.accepts(file));
  // full transcript가 있으면 짧은 preview 파일을 중복 세션으로 올리지 않는다.
  return source.provider === "antigravity" ? unique.filter((file) => !file.endsWith("/transcript.jsonl") || !unique.includes(file.replace(/transcript\.jsonl$/, "transcript_full.jsonl"))) : unique;
}

export async function readAdditionalFileSessions(context: AiSessionReaderContext): Promise<AiSessionReaderResult[]> {
  const home = await getHomeDirectory(context.sshHost);
  const scope = createScope(home, context);
  return mapWithConcurrency(fileSources(home, Boolean(context.sshHost)), PROVIDER_CONCURRENCY, async (source) => {
    try {
      const available = (await Promise.all(source.roots.map((root) => pathExists(root, context.sshHost)))).some(Boolean);
      if (!available) return createReaderResult(source.provider, { available: false, reason: "Session directory not found" });
      const files = await discoverSource(source, context);
      let brokenFiles = 0;
      const parsed = await mapWithConcurrency(files, FILE_CONCURRENCY, async (file) => {
        try {
          if (!await allowedSource(file, source, context)) return null;
          return await readSession(scope, source.provider, file);
        } catch {
          // 하나의 손상 파일이나 잠긴 sidecar가 다른 Agent 기록까지 숨기지 않게 한다.
          brokenFiles += 1;
          return null;
        }
      });
      const sessions = parsed.flatMap((entry) => entry ? [toSummary(entry.session, source.provider, entry.file)] : []);
      return createReaderResult(source.provider, { sessions, reason: brokenFiles ? `${brokenFiles} session files could not be read` : null });
    } catch {
      // 저장소 접근 실패는 이 source에만 표시하고 나머지 source 조회를 계속한다.
      return createReaderResult(source.provider, { available: false, reason: "Session storage could not be read" });
    }
  });
}

export async function readAdditionalFileSessionDetail(
  context: AiSessionReaderContext, provider: AiSessionProvider, sessionId: string,
  sourceRef?: string | null, cursor?: string | null, limit = DEFAULT_DETAIL_LIMIT,
): Promise<AiSessionDetailReaderResult | null> {
  const home = await getHomeDirectory(context.sshHost);
  const source = fileSources(home, Boolean(context.sshHost)).find((entry) => entry.provider === provider);
  if (!source) return null;
  if (sourceRef && !await allowedSource(sourceRef, source, context)) return null;
  const files = sourceRef ? [sourceRef] : await discoverSource(source, context);
  const scope = createScope(home, context);
  for (const file of files) {
    try {
      if (!await allowedSource(file, source, context)) continue;
      const parsed = await readSession(scope, provider, file);
      if (!parsed || parsed.session.id !== sessionId) continue;
      const { session } = parsed;
      const messages = session.messages.filter((message) => (!context.roles?.length || context.roles.includes(message.role)) && (!context.query || message.fullText.toLowerCase().includes(context.query.toLowerCase())));
      // 일부 Agent는 assistant·tool 시각을 저장하지 않으므로 로그의 기록 순서를 유지한다.
      const page = paginateItems([...messages].reverse(), cursor, limit);
      return createSessionDetail({ sessionId, provider, title: session.title ?? session.messages.find((message) => message.role === "user")?.text ?? null, matchedPath: session.cwd, sourceRef: file, messages: page.items, nextCursor: page.nextCursor });
    } catch {
      // 한 후보의 부분 기록으로 상세 조회 전체를 실패시키지 않고 다음 후보를 확인한다.
    }
  }
  return null;
}
async function readSession(scope: ReadScope, provider: AiSessionProvider, file: string): Promise<{ session: AdditionalFileSession; file: string } | null> {
  if (HEADER_SCOPED_PROVIDERS.has(provider)) {
    const head = await getCachedOrParseHead(file, () => readJsonLinesHead(file, SESSION_METADATA_HEAD_LINES, scope.context.sshHost), scope.context.sshHost);
    const cwd = decodeAdditionalFileSession(provider, file, head).cwd;
    if (cwd && !determineMatchScope(cwd, scope.context)) return null;
  }
  if (provider === "cursor") {
    const metadata = await readCursorMetadata(scope, file);
    const cwd = sessionString(metadata.cwd);
    if (cwd && !determineMatchScope(cwd, scope.context)) return null;
  }
  const raw = await readParsedFile(scope, file);
  if (!raw) return null;
  if ((provider === "pi" || provider === "omp") && !matchesGraphWriter(provider, file, raw, scope)) return null;
  const sidecar = await readSidecar(scope, provider, file, raw);
  if (provider === "devin" && Boolean(sidecar.hidden)) return null;
  const session = decodeAdditionalFileSession(provider, file, raw, sidecar);
  if (!determineMatchScope(session.cwd, scope.context)) return null;
  if (scope.context.query) {
    const query = scope.context.query.toLowerCase();
    if (![session.id, session.title, session.cwd, ...session.messages.map((message) => message.fullText)].some((text) => text?.toLowerCase().includes(query))) return null;
  }
  return { session, file };
}
function matchesGraphWriter(provider: "pi" | "omp", file: string, raw: unknown, scope: ReadScope): boolean {
  const ownOmpRoot = fileSources(scope.home, Boolean(scope.context.sshHost)).find((source) => source.provider === "omp")!.roots[0];
  const piRoot = fileSources(scope.home, Boolean(scope.context.sshHost)).find((source) => source.provider === "pi")!.roots[0];
  const ompRoot = isWithin(file, ownOmpRoot) && ownOmpRoot !== piRoot;
  const records = Array.isArray(raw) ? raw.map(sessionRecord) : [];
  const ompWriter = ompRoot || records.some((record) => (record.runtime !== null && typeof record.runtime === "object" && !Array.isArray(record.runtime)) || (record.type === "title" && record.v === 1) || (record.type === "model_change" && record.modelId === undefined && typeof record.model === "string"));
  return provider === "omp" ? ompWriter : !ompWriter;
}
function toSummary(session: AdditionalFileSession, provider: AiSessionProvider, file: string): AggregatedAiSession {
  const firstUserPrompt = session.messages.find((message) => message.role === "user")?.fullText ?? null;
  return { id: session.id, provider, matchedPath: session.cwd, matchScope: "worktree", startedAt: session.startedAt, updatedAt: session.updatedAt, title: session.title ? truncateText(session.title, 80) : firstUserPrompt ? truncateText(firstUserPrompt, 80) : null, firstUserPrompt: firstUserPrompt ? truncateText(firstUserPrompt) : null, messageCount: session.messages.length, sourceRef: file };
}

async function readSidecar(scope: ReadScope, provider: AiSessionProvider, file: string, raw: unknown): Promise<Record<string, unknown>> {
  const directory = path.dirname(file);
  switch (provider) {
    case "grok": return { lines: await optionalFile(scope, path.join(directory, "chat_history.jsonl")) };
    case "rovo": return sessionRecord(await optionalFile(scope, path.join(directory, "session_context.json")));
    case "cline": return sessionRecord(await optionalFile(scope, file.replace(/\.json$/, ".messages.json")));
    case "kiro": return { lines: await optionalFile(scope, `${file}l`) };
    case "cursor": return readCursorMetadata(scope, file);
    case "antigravity": return readAntigravityWorkspace(scope, file, raw);
    case "devin": {
      const dbPath = path.join(path.dirname(directory), "sessions.db");
      let pending = scope.devinIndexes.get(dbPath);
      if (!pending) { pending = readDevinSessionIndex(dbPath, scope.context.sshHost); scope.devinIndexes.set(dbPath, pending); }
      const id = sessionString(sessionRecord(raw).session_id) ?? sessionString(sessionRecord(raw).sessionId) ?? path.basename(file, ".json");
      return (await pending).get(id) ?? {};
    }
    case "kimi": {
      const sessionsRoot = path.dirname(path.dirname(directory));
      const index = await optionalFile(scope, path.join(path.dirname(sessionsRoot), "session_index.jsonl"));
      const sessionId = path.basename(directory);
      const entry = Array.isArray(index) ? index.map(sessionRecord).findLast((record) => record.sessionId === sessionId) : undefined;
      const agents = sessionRecord(sessionRecord(raw).agents);
      const primary = Object.entries(agents).find(([, value]) => sessionRecord(value).type === "main" && sessionRecord(value).parentAgentId == null)?.[0] ?? "main";
      // state.json의 agent ID가 경로 밖을 지시하지 못하게 한다.
      if (!/^[\w-]+$/.test(primary)) return {};
      return { workDir: entry?.workDir, lines: await optionalFile(scope, path.join(directory, "agents", primary, "wire.jsonl")) };
    }
    default: return {};
  }
}
async function readCursorMetadata(scope: ReadScope, transcript: string): Promise<Record<string, unknown>> {
  const cursorRoot = path.join(scope.home, ".cursor");
  const id = path.basename(path.dirname(transcript)) === "agent-transcripts" ? path.basename(transcript, ".jsonl") : path.basename(path.dirname(transcript));
  scope.cursorMetadataFiles ??= listFilesRecursivelyBySuffix(path.join(cursorRoot, "chats"), ".json", scope.context.sshHost);
  const files = await scope.cursorMetadataFiles;
  const matches = files.filter((file) => path.basename(file) === "meta.json" && path.basename(path.dirname(file)) === id);
  if (matches.length !== 1) return {};
  return sessionRecord(await readParsedFile(scope, matches[0]));
}
async function readAntigravityWorkspace(scope: ReadScope, file: string, raw: unknown): Promise<Record<string, unknown>> {
  const root = path.join(scope.home, ".gemini", "antigravity-cli");
  const id = path.basename(path.dirname(path.dirname(path.dirname(file))));
  const history = await optionalFile(scope, path.join(root, "history.jsonl"));
  const records = Array.isArray(history) ? history.map(sessionRecord) : [];
  const paths = new Set(records.filter((record) => record.conversationId === id).map((record) => sessionString(record.workspace)).filter((value): value is string => value !== null));
  const [metadata, projects, lastConversations] = await Promise.all(["conversation_metadata.json", "projects.json", "last_conversations.json"].map((name) => optionalFile(scope, path.join(root, "cache", name))));
  const last = sessionRecord(lastConversations);
  for (const [workspace, conversationId] of Object.entries(last)) if (conversationId === id) paths.add(workspace);
  const projectId = sessionRecord(sessionRecord(sessionRecord(metadata).conversations)[id]).summary;
  const targetProject = sessionString(sessionRecord(projectId).ProjectID);
  if (targetProject) for (const [key, value] of Object.entries(sessionRecord(projects))) {
    if (key === targetProject && typeof value === "string" && path.isAbsolute(value)) paths.add(value);
    if (value === targetProject && path.isAbsolute(key)) paths.add(key);
  }
  if (!paths.size) {
    const transcript = decodeAdditionalFileSession("antigravity", file, raw);
    const first = transcript.messages.find((message) => message.role === "user");
    if (first?.timestamp) for (const record of records) {
      const timestamp = typeof record.timestamp === "number" && record.timestamp < 1e12 ? record.timestamp * 1000 : Date.parse(String(record.timestamp));
      if (record.display === first.fullText && Math.abs(timestamp - Date.parse(first.timestamp)) <= ANTIGRAVITY_HISTORY_MATCH_WINDOW_MS) {
        const workspace = sessionString(record.workspace); if (workspace) paths.add(workspace);
      }
    }
  }
  // 동일 ID가 서로 다른 작업에 연결되면 임의의 cwd를 택하지 않는다.
  return paths.size === 1 ? { workspace: [...paths][0] } : {};
}
