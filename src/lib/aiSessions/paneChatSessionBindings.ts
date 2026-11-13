import { execGit } from "@/lib/gitOperations";
import { isPaneChatProviderProcess } from "./paneChatProcess";
import { readClaudeSessions } from "./readClaudeSessions";
import { readCodexSessions } from "./readCodexSessions";
import { determineMatchScope } from "./shared";
import type { AiSessionReaderContext } from "./types";

type ChatProvider = "claude" | "codex";
interface BindingContext extends AiSessionReaderContext { taskId: string }
interface BindingQuery {
  taskId: string;
  sshHost?: string | null;
  provider: ChatProvider;
  providerPid: number;
  processStartTime: string;
}
interface SessionBinding {
  context: BindingContext;
  provider: ChatProvider;
  sessionId: string;
  recordedAt: number;
  sourceRef?: string;
}
interface ProcessRow { pid: number; parentPid: number; startTime: string; command: string; args: string }

const MAX_BINDINGS = 256;
const BINDING_TTL_MS = 24 * 60 * 60 * 1_000;
const MAX_HOOK_BASE64_LENGTH = 96 * 1_024;
const MAX_ANCESTRY_DEPTH = 32;
const PROCESS_PROBE_TIMEOUT_MS = 3_000;
const bindings = new Map<string, SessionBinding>();

function bindingKey(query: BindingQuery): string {
  return JSON.stringify([query.taskId, query.sshHost ?? "", query.provider, query.providerPid, query.processStartTime.trim().replace(/\s+/g, " ")]);
}

function findProviderAncestor(output: string, hookPid: number, provider: ChatProvider): ProcessRow | null {
  const processes = new Map<number, ProcessRow>();
  for (const line of output.split("\n")) {
    const match = line.match(/^\s*(\d+)\s+(\d+)\s+(\S+\s+\S+\s+\S+\s+\S+\s+\S+)\s+(\S+)(?:\s+(.*))?$/);
    if (!match) continue;
    processes.set(Number(match[1]), { pid: Number(match[1]), parentPid: Number(match[2]), startTime: match[3].replace(/\s+/g, " "), command: match[4], args: match[5] ?? "" });
  }
  const visited = new Set<number>();
  let pid = hookPid;
  for (let depth = 0; depth < MAX_ANCESTRY_DEPTH && pid > 1 && !visited.has(pid); depth++) {
    visited.add(pid);
    const row = processes.get(pid);
    if (!row) return null;
    if (isPaneChatProviderProcess(row.command, row.args, provider)) return row;
    pid = row.parentPid;
  }
  return null;
}

/** 대화 ID는 hook에서 받고, 실행 ID는 실제 에이전트 조상 프로세스에서만 얻는다. */
export async function recordPaneChatSessionBinding(context: BindingContext, metadata: unknown): Promise<void> {
  if (!metadata || typeof metadata !== "object") return;
  const input = metadata as Record<string, unknown>;
  if ((input.provider !== "claude" && input.provider !== "codex")
    || !Number.isSafeInteger(input.hookPid) || Number(input.hookPid) <= 1
    || typeof input.payloadBase64 !== "string" || input.payloadBase64.length > MAX_HOOK_BASE64_LENGTH) return;
  try {
    const payload = JSON.parse(Buffer.from(input.payloadBase64, "base64").toString("utf8")) as Record<string, unknown>;
    if (!payload || typeof payload !== "object" || Array.isArray(payload)
      || (input.provider === "codex" && payload.agent_id)
      || typeof payload.session_id !== "string" || !/^[A-Za-z0-9._:-]{1,200}$/.test(payload.session_id)
      || typeof payload.cwd !== "string" || !determineMatchScope(payload.cwd, context)) return;
    const output = await execGit("ps -wwaxo pid=,ppid=,lstart=,ucomm=,args=", context.sshHost, { timeoutMs: PROCESS_PROBE_TIMEOUT_MS });
    const process = findProviderAncestor(output, Number(input.hookPid), input.provider);
    if (!process) return;
    const now = Date.now();
    for (const [key, entry] of bindings) {
      if (now - entry.recordedAt > BINDING_TTL_MS) bindings.delete(key);
    }
    const key = bindingKey({ taskId: context.taskId, sshHost: context.sshHost, provider: input.provider, providerPid: process.pid, processStartTime: process.startTime });
    const previous = bindings.get(key);
    bindings.delete(key);
    bindings.set(key, { context: { ...context }, provider: input.provider, sessionId: payload.session_id, recordedAt: now,
      ...(previous?.sessionId === payload.session_id && previous.sourceRef ? { sourceRef: previous.sourceRef } : {}) });
    while (bindings.size > MAX_BINDINGS) bindings.delete(bindings.keys().next().value!);
  } catch {
    // 채팅 메타데이터를 얻지 못해도 기존 태스크 상태 hook은 성공해야 한다.
  }
}

/** 호출자가 현재 pane의 PID와 시작 시각을 검증한 뒤 조회한다. */
export async function findPaneChatSessionBinding(query: BindingQuery): Promise<{ sessionId: string; sourceRef: string } | null> {
  const key = bindingKey(query);
  const entry = bindings.get(key);
  if (!entry || Date.now() - entry.recordedAt > BINDING_TTL_MS) return null;
  if (!entry.sourceRef) {
    // HTTP hook이 보낸 경로는 열지 않고, 기존 공급자 탐색이 허용한 파일만 사용한다.
    const context = { ...entry.context, query: entry.sessionId };
    const page = await (entry.provider === "claude" ? readClaudeSessions(context) : readCodexSessions(context));
    if (bindings.get(key) !== entry) return null;
    const exact = page.sessions.find((session) => session.id === entry.sessionId && session.sourceRef);
    if (!exact?.sourceRef) return null;
    entry.sourceRef = exact.sourceRef;
  }
  return { sessionId: entry.sessionId, sourceRef: entry.sourceRef };
}
