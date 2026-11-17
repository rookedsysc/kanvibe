import { createHash, randomUUID } from "node:crypto";
import path from "node:path";
import { SessionType } from "@/entities/KanbanTask";
import { execGit } from "@/lib/gitOperations";
import { focusHerdrTaskPane, listLocalTerminalTabs } from "@/lib/terminal";
import { quoteForPosixShell } from "@/lib/worktree";
import { getHomeDirectory, pathExists, readTextFile } from "@/lib/hostFileAccess";
import { MAX_ATTACHMENT_BYTES } from "@/lib/attachmentContent";
import { getCachedOrParseTail, readJsonLinesHead, readJsonLinesTail } from "@/lib/aiSessions/shared";
import { buildPaneChatModelCommand, buildPaneChatModelSettings, parseCodexModelCatalog, parsePaneChatModelSettingChange, readClaudeModelState, readCodexModelState, type PaneChatModelOption, type PaneChatModelSettings } from "@/lib/aiSessions/paneChatModelSettings";
import { readClaudeSessionDetail } from "@/lib/aiSessions/readClaudeSessions";
import { readGeminiSessionDetail } from "@/lib/aiSessions/readGeminiSessions";
import { readOpenCodeSessionDetail } from "@/lib/aiSessions/readOpenCodeSessions";
import { findCodexMainThreadRollout, readCodexSessionDetail } from "@/lib/aiSessions/readCodexSessions";
import { expectCodexComposerTexts, isCodexMainComposerReady, isCodexThreadFooterSupported, isSameComposerText, readCodexDisplay } from "@/lib/aiSessions/paneChatCodexDisplay";
import { readCodexEmptyThread } from "@/lib/aiSessions/codexThreadMetadata";
import { sendCodexThreadInput } from "@/lib/aiSessions/codexThreadInput";
import { readCodexThreadSettings, updateCodexThreadSettings } from "@/lib/aiSessions/codexThreadSettings";
import { findPaneChatProcess, type PaneChatProcess } from "@/lib/aiSessions/paneChatProcess";
import { findPaneChatSessionBinding, findPaneChatProcessSessionBinding, findPaneChatReportedSessionBinding } from "@/lib/aiSessions/paneChatSessionBindings";
import { MAX_PANE_CHAT_ATTACHMENTS, MAX_PANE_CHAT_CATALOG_SUGGESTIONS, type PaneChatActionResult, type PaneChatAttachment, type PaneChatCodexThreadReveal, type PaneChatAttachmentInput, type PaneChatKey, type PaneChatLayout, type PaneChatSnapshot, type PaneChatSuggestionKind, type PaneChatSuggestions } from "@/desktop/shared/paneChat";
import { readPaneChatLayout, readPaneChatScreen, readPaneChatSurface, type PaneChatSurface } from "./paneChatSurface";
import { discardPaneChatAttachment, fingerprintPaneChatAttachments, resolvePaneChatAttachments, retainPaneChatAttachments, storePaneChatAttachment, type PaneChatAttachmentOwner } from "./paneChatAttachmentService";
import { readPaneChatSuggestions } from "./paneChatComposerCatalog";
import { writeToPane } from "./mobileBridgeService";

export { getPaneChatModes, setPaneChatMode } from "./paneChatModeStore";

const PROCESS_TABLE_COLUMNS = "pid=,ppid=,pgid=,tpgid=,tty=,lstart=,ucomm=,args=";
/** 사용자가 버튼을 눌렀을 때만 Codex 하단 상태줄을 대화 ID로 바꾸는 명령. 그 TUI 세션에만 적용되고 설정 파일에는 저장되지 않는다 */
const CODEX_THREAD_FOOTER_COMMAND = "/statusline thread-id";
/** 붙여넣은 명령이 입력란에 보이거나, 실행 뒤 대화 ID가 footer에 나타나기까지 화면을 다시 읽는 횟수와 간격 */
const CODEX_FOOTER_READ_ATTEMPTS = 20;
const CODEX_FOOTER_READ_INTERVAL_MS = 50;
const PROCESS_PROBE_TIMEOUT_MS = 5_000;
const MAX_PROMPT_BYTES = 64 * 1024;
const MAX_TARGET_LEASES = 256;
const TARGET_LEASE_TTL_MS = 10 * 60 * 1_000;
const TRANSCRIPT_PAGE_LIMIT = 100;
const transcriptReaders = { claude: readClaudeSessionDetail, codex: readCodexSessionDetail, gemini: readGeminiSessionDetail, opencode: readOpenCodeSessionDetail };
/** 현재 모델 설정을 찾으려고 기록의 끝에서 읽는 줄 수. 최근 응답 하나만 들어 있으면 된다 */
const MODEL_STATE_TAIL_LINES = 200;
/** Codex 모델 목록은 CLI가 가끔 갱신하는 캐시라, 매번 다시 읽지 않고 이 시간 동안 재사용한다 */
const CODEX_MODEL_CATALOG_TTL_MS = 60_000;
/** 구독 감시가 읽은 결과를 같은 pane의 데스크톱·HTTP 조회가 재사용하는 최대 경과 시간. 구독 감시가 pane을 다시 읽는 주기와 같다 */
const SHARED_READING_MAX_AGE_MS = 1_000;
// Orca CLI bridge와 같은 간격으로 붙여넣기 처리가 끝난 뒤 Enter를 보내고, 에코를 확인하며 치는 명령은 한 글자씩 친다.
const PROMPT_SUBMIT_DELAY_MS = 500;
const PICKER_CHARACTER_DELAY_MS = 40;
const IMAGE_ATTACHMENT_SETTLE_MS = 300;
const KEY_BYTES: Record<PaneChatKey, string> = { up: "\x1b[A", down: "\x1b[B", left: "\x1b[D", right: "\x1b[C", enter: "\r", escape: "\x1b" };
/** Claude가 작업 중일 때만 입력란 아래 footer에 붙이는 중단 안내 */
const CLAUDE_WORKING_FOOTER_HINT = /\besc to interrupt\b/;
/** Codex가 작업 중일 때만 입력란 위에 띄우는 상태 줄. 예: "• Working (3s • esc to interrupt)" */
const CODEX_WORKING_STATUS_LINE = /^•\s.*\(.*\besc to interrupt\)$/;
/** Codex 상태 줄과 입력란 사이에는 사용량 경고 같은 줄이 끼어들 수 있어 입력란 위로 이만큼 찾는다 */
const CODEX_WORKING_STATUS_SEARCH_ROWS = 6;

interface TargetLease {
  taskId: string;
  requestedPaneId?: string;
  paneId: string;
  identity: string;
  process: PaneChatProcess;
  conversation: string | null;
  conversationKey: string | null;
  expiresAt: number;
}
const leases = new Map<string, TargetLease>();
const mutations = new Map<string, Promise<unknown>>();
const receipts = new Map<string, Map<string, { fingerprint: string; result: Promise<PaneChatActionResult> }>>();
interface CodexDisplayExecution {
  observedThread: string | null;
  revision: number;
  reason: string;
  observation: number;
}
const codexDisplays = new Map<string, Promise<CodexDisplayExecution | null>>();

async function codexDisplayExecution(surface: PaneChatSurface, process: PaneChatProcess): Promise<CodexDisplayExecution | null> {
  if (process.provider !== "codex" || surface.task.sessionType === SessionType.ZELLIJ) return null;
  const identity = identityFor(surface, process);
  let state = codexDisplays.get(identity);
  if (!state) {
    state = (async () => {
      try {
        const output = await execGit(`lsof -a -p ${process.pid} -d txt -Fn`, surface.task.sshHost, { timeoutMs: PROCESS_PROBE_TIMEOUT_MS });
        const binaries = [...new Set(output.split("\n").filter((line) => line.startsWith("n/") && line.endsWith("/codex")).map((line) => line.slice(1)))];
        if (binaries.length !== 1) return null;
        const version = await execGit(`${quoteForPosixShell(binaries[0])} --version`, surface.task.sshHost, { timeoutMs: PROCESS_PROBE_TIMEOUT_MS });
        return isCodexThreadFooterSupported(version) ? { observedThread: null, revision: 0, reason: "codex-thread-unconfirmed", observation: 0 } : null;
      } catch { return null; }
    })();
    if (codexDisplays.size >= MAX_TARGET_LEASES) codexDisplays.delete(codexDisplays.keys().next().value!);
    codexDisplays.set(identity, state);
  }
  return state;
}
function codexDisplay(surface: PaneChatSurface) {
  return surface.viewportLines && Number.isInteger(surface.cursorRow) ? readCodexDisplay(surface.viewportLines, surface.cursorRow!, surface.inputIsPlaceholder === true) : null;
}
function observeCodexThread(state: CodexDisplayExecution, id: string | null, observation?: number): void {
  if (observation !== undefined && observation < state.observation && id !== state.observedThread) throw changedTargetError();
  state.observation = Math.max(state.observation, observation ?? state.observation);
  if (state.observedThread === id) return;
  state.observedThread = id;
  state.revision++;
}

async function sameExecutionSurface(taskId: string, paneId: string, identity: string): Promise<PaneChatSurface> {
  const surface = await readPaneChatSurface(taskId, paneId);
  const process = surface ? await inspectProcess(surface) : null;
  if (!surface || !process || identityFor(surface, process) !== identity) throw changedTargetError();
  return surface;
}


function inputError(message: string): Error {
  return Object.assign(new Error(message), { name: "PaneChatInputError" });
}
function changedTargetError(): Error {
  return Object.assign(new Error("CLI 대상이 변경되었습니다. 현재 pane을 다시 확인해 주세요."), { name: "PaneChatTargetChangedError" });
}
function identityFor(surface: PaneChatSurface, process: PaneChatProcess): string {
  return JSON.stringify([surface.task.id, surface.task.sshHost, surface.task.sessionType, surface.task.sessionName, surface.task.worktreePath, surface.task.project?.repoPath, surface.paneId, surface.rootPid, process.provider, process.pid, process.processStartTime, process.codexServerSocket]);
}
function conversationKeyFor(surface: PaneChatSurface, process: PaneChatProcess, conversation: string | null): string | null {
  if (!conversation) return null;
  const [sessionId, sourceRef] = JSON.parse(conversation) as [string, string, number];
  return createHash("sha256").update(JSON.stringify([surface.task.id, surface.task.sshHost, surface.task.sessionType, surface.task.sessionName, surface.task.worktreePath, surface.task.project?.repoPath, surface.paneId, process.provider, sessionId, sourceRef])).digest("hex");
}
async function inspectProcess(surface: PaneChatSurface): Promise<PaneChatProcess | null> {
  if (!surface.rootPid) return null;
  return findPaneChatProcess(await execGit(buildPaneProcessTableCommand(surface.rootPid), surface.task.sshHost, { timeoutMs: PROCESS_PROBE_TIMEOUT_MS }), surface.rootPid);
}
/**
 * pane root와 같은 터미널에 붙은 프로세스만 조회한다. 판정은 root의 tty에 있는 자손만 보므로 전체 프로세스 표를 읽을 필요가 없다.
 * tty가 없거나 확인되지 않으면 아무것도 출력하지 않아 "CLI 없음"으로 판정된다.
 */
function buildPaneProcessTableCommand(rootPid: number): string {
  return `pane_tty=$(ps -o tty= -p ${rootPid} | tr -d ' '); case "$pane_tty" in ""|"?"|"??") ;; *) ps -wwo ${PROCESS_TABLE_COLUMNS} -t "$pane_tty" ;; esac`;
}
function retireChangedLeases(taskId: string, paneId: string, identity: string, conversation: string | null): void {
  for (const [token, lease] of leases) {
    if (lease.taskId === taskId && lease.paneId === paneId && (lease.identity !== identity || lease.conversation !== conversation)) {
      leases.delete(token);
      receipts.delete(token);
    }
  }
}
function leaseTarget(surface: PaneChatSurface, process: PaneChatProcess, requestedPaneId: string | undefined, conversation: string | null): string {
  const now = Date.now();
  const identity = identityFor(surface, process);
  for (const [token, lease] of leases) {
    if (lease.expiresAt < now) { leases.delete(token); receipts.delete(token); }
    else if (lease.identity === identity && lease.requestedPaneId === requestedPaneId && lease.conversation === conversation) {
      lease.expiresAt = now + TARGET_LEASE_TTL_MS;
      return token;
    }
  }
  if (leases.size >= MAX_TARGET_LEASES) {
    const oldest = leases.keys().next().value!;
    leases.delete(oldest);
    receipts.delete(oldest);
  }
  const token = randomUUID();
  leases.set(token, { taskId: surface.task.id, requestedPaneId, paneId: surface.paneId, identity, process, conversation, conversationKey: conversationKeyFor(surface, process, conversation), expiresAt: now + TARGET_LEASE_TTL_MS });
  return token;
}
function readInputState(surface: PaneChatSurface, process: PaneChatProcess, verifyCodexEmpty = false): PaneChatSnapshot["inputState"] {
  if (surface.inputLine === null || surface.cursorColumn === null) return "unknown";
  if (process.provider === "gemini" || process.provider === "opencode") {
    const line = surface.inputLine;
    const marker = process.provider === "gemini" ? line.match(/^\s*> /) : line.match(/^\s*┃ {2}/);
    if (!marker) return "unknown";
    const inputColumn = process.provider === "gemini" ? line.indexOf(">") + 2 : marker[0].length;
    const value = line.slice(inputColumn).trim();
    const placeholder = process.provider === "gemini" ? /^Type your message(?: or @path\/to\/file)?$/.test(value) : /^Ask anything…(?:\s|$)/.test(value);
    const lines = surface.viewportLines;
    const row = surface.cursorRow;
    if (!lines || row === null || row === undefined || lines[row] !== line) return "unknown";
    if (process.provider === "gemini") {
      const border = /^─{3,}\s*$/;
      if (row < 1 || !border.test(lines[row - 1])) return "unknown";
      const end = lines.findIndex((content, index) => index > row && border.test(content));
      if (end < 0) return "unknown";
      if (lines.slice(row + 1, end).some((content) => content.trim())) return "occupied";
    } else {
      const prefix = line.slice(0, line.indexOf("┃") + 1);
      let start = row;
      let end = row;
      while (start > 0 && lines[start - 1].startsWith(prefix)) start--;
      while (end + 1 < lines.length && lines[end + 1].startsWith(prefix)) end++;
      if (!lines[end + 1]?.startsWith(`${prefix.slice(0, -1)}╹▀`) || end <= start || !lines[end].includes(" · ")) return "unknown";
      const occupied = lines.slice(start, end).some((content, index) => {
        const text = content.slice(inputColumn).trim();
        return !!text && !(start + index === row && placeholder);
      });
      if (occupied || row === end) return "occupied";
    }
    return surface.cursorColumn === inputColumn && (!value || placeholder) ? "ready" : "occupied";
  }
  if (process.provider === "codex") {
    const text = codexDisplay(surface)?.composerText;
    if (verifyCodexEmpty && surface.inputIsPlaceholder === true && surface.cursorColumn === 2 && surface.inputLine.startsWith("› ") && surface.viewportLines && isCodexMainComposerReady(surface.viewportLines, surface.cursorRow ?? -1)) return "ready";
    if (text !== undefined && text !== "" && text !== "Ask Codex to do anything") return "occupied";
    if (verifyCodexEmpty && (text === "" || text === "Ask Codex to do anything")) return "occupied";
  }
  const marker = process.provider === "claude" ? "❯" : process.provider === "codex" ? "›" : null;
  if (!marker) return "unknown";
  const line = surface.inputLine;
  const start = line.indexOf(marker);
  if (start < 0 || line.slice(0, start).trim()) return "unknown";
  const value = line.slice(start + marker.length).trim();
  const isPlaceholder = surface.inputIsPlaceholder === true || (process.provider === "codex" ? value === "Ask Codex to do anything" : /^Try\s+/.test(value));
  return surface.cursorColumn === start + marker.length + 1 && (!value || isPlaceholder) ? "ready" : "occupied";
}
/**
 * 에이전트가 응답을 만들거나 도구를 실행하는 중인지 화면 표시로 판정한다.
 * Claude 2.1.325와 Codex 0.169에서 작업 중에만 나타나고 끝나면 사라지는 것을 확인한 자리만 본다.
 * 대화 본문에 같은 문구가 있어도 속지 않게 입력란 기준 위치로 좁히고, 확인하지 않은 CLI나 화면 좌표가 없는 surface는 unknown이다.
 */
function readAgentActivity(surface: PaneChatSurface, process: PaneChatProcess): PaneChatSnapshot["agentActivity"] {
  const lines = surface.viewportLines;
  const inputRow = surface.cursorRow;
  if (!lines || inputRow === null || inputRow === undefined || inputRow < 0 || inputRow >= lines.length) return "unknown";
  if (process.provider === "claude") {
    const footer = lines.slice(inputRow + 1);
    return footer.some((line) => CLAUDE_WORKING_FOOTER_HINT.test(line)) ? "working" : "idle";
  }
  if (process.provider === "codex") {
    const aboveComposer = lines.slice(Math.max(0, inputRow - CODEX_WORKING_STATUS_SEARCH_ROWS), inputRow);
    return aboveComposer.some((line) => CODEX_WORKING_STATUS_LINE.test(line.trim())) ? "working" : "idle";
  }
  return "unknown";
}
async function requireReadyInput(taskId: string, lease: TargetLease): Promise<void> {
  const surface = await readPaneChatSurface(taskId, lease.requestedPaneId);
  if (!surface || readInputState(surface, lease.process, !!await codexDisplayExecution(surface, lease.process)) !== "ready") throw inputError("CLI 입력란이 비어 있지 않거나 선택 화면이 열려 있습니다. 터미널에서 현재 상태를 확인해 주세요.");
}

function emptyTranscript(reason: string): PaneChatSnapshot["transcript"] {
  return { status: "unavailable", reason, sessionId: null, messages: [], nextCursor: null };
}
interface ReadTranscript {
  transcript: PaneChatSnapshot["transcript"];
  /** 대화를 읽어 온 JSONL 파일. 연결하지 못했으면 null이다 */
  sourceRef: string | null;
}
async function resolveConversation(surface: PaneChatSurface, process: PaneChatProcess): Promise<{ sessionId: string; sourceRef: string; revision?: number; pending?: boolean; empty?: boolean } | null> {
  const { task } = surface;
  const context = { worktreePath: task.worktreePath ?? task.project?.repoPath ?? null, repoPath: task.project?.repoPath ?? null, sshHost: task.sshHost };
  const displayed = await codexDisplayExecution(surface, process);
  const id = displayed ? codexDisplay(surface)?.threadId : null;
  if (displayed && id) {
    if (displayed.observedThread !== id && readInputState(surface, process, true) !== "ready") {
      observeCodexThread(displayed, null, surface.observation);
      displayed.reason = "codex-thread-input-busy";
      return null;
    }
    let sourceRef = await findCodexMainThreadRollout(context, id);
    const pending = !sourceRef;
    sourceRef ??= await readCodexEmptyThread(context, id, process.codexServerSocket);
    if (!sourceRef) { observeCodexThread(displayed, null, surface.observation); displayed.reason = "codex-thread-record-missing"; return null; }
    observeCodexThread(displayed, id, surface.observation);
    return { sessionId: id, sourceRef, revision: displayed.revision, ...(pending ? { pending: true } : {}) };
  }
  if (displayed) {
    observeCodexThread(displayed, null, surface.observation);
    displayed.reason = readInputState(surface, process, true) === "ready" ? "codex-thread-unconfirmed" : "codex-thread-input-busy";
  }
  let binding = await findPaneChatProcessSessionBinding(context, process);
  if (process.provider === "opencode") return binding;
  binding ??= await findPaneChatSessionBinding({ taskId: task.id, sshHost: task.sshHost, provider: process.provider, providerPid: process.pid, processStartTime: process.processStartTime });
  if (!binding && surface.agentSession?.provider === process.provider) {
    binding = await findPaneChatReportedSessionBinding(context, process.provider, surface.agentSession.sessionId);
    if (!binding && process.provider === "codex") {
      const sourceRef = await readCodexEmptyThread(context, surface.agentSession.sessionId, process.codexServerSocket);
      if (sourceRef) binding = { sessionId: surface.agentSession.sessionId, sourceRef, pending: true };
    }
  }
  if (!binding && (process.provider === "claude" || process.provider === "codex")) {
    try {
      const output = await execGit(`lsof -a -p ${process.pid} -Fn`, task.sshHost, { timeoutMs: PROCESS_PROBE_TIMEOUT_MS });
      const home = await getHomeDirectory(task.sshHost);
      const root = process.provider === "claude" ? path.posix.join(home, ".claude", "projects") : path.posix.join(home, ".codex", "sessions");
      const candidates = [...new Set(output.split("\n").filter((line) => line.startsWith(`n${root}/`) && line.endsWith(".jsonl")).map((line) => line.slice(1)))];
      const exactBindings: { sessionId: string; sourceRef: string }[] = [];
      for (const sourceRef of candidates) {
        const records = await readJsonLinesHead(sourceRef, TRANSCRIPT_PAGE_LIMIT, task.sshHost) as { sessionId?: string; type?: string; payload?: { id?: string } }[];
        const sessionId = process.provider === "claude" ? records.find((record) => typeof record.sessionId === "string")?.sessionId : records.find((record) => record.type === "session_meta")?.payload?.id;
        const meta = records.find((record) => record.type === "session_meta")?.payload as Record<string, unknown> | undefined;
        if (process.provider === "codex" && meta?.source && typeof meta.source === "object" && "subagent" in meta.source) continue;
        if (sessionId) exactBindings.push({ sessionId, sourceRef });
      }
      if (exactBindings.length === 1) binding = exactBindings[0];
    } catch {
      // CLI가 쓰기 사이에 JSONL을 닫을 수 있다. 열린 파일이 없으면 다른 기록을 연결하지 않는다.
    }
  }
  return binding;
}
function conversationIdentity(binding: { sessionId: string; sourceRef: string; revision?: number } | null): string | null {
  return binding ? JSON.stringify([binding.sessionId, binding.sourceRef, binding.revision ?? null]) : null;
}
async function readTranscript(surface: PaneChatSurface, process: PaneChatProcess, binding: { sessionId: string; sourceRef: string; revision?: number; pending?: boolean; empty?: boolean } | null): Promise<ReadTranscript> {
  const unavailable = (reason: string): ReadTranscript => ({ transcript: emptyTranscript(reason), sourceRef: null });
  const { task } = surface;
  if (!binding) return unavailable("exact-session-unavailable");
  if (binding.empty) return { transcript: { status: "available", reason: null, sessionId: binding.sessionId, messages: [], nextCursor: null }, sourceRef: null };
  if (binding.pending && !await pathExists(binding.sourceRef, task.sshHost)) return { transcript: { status: "available", reason: null, sessionId: binding.sessionId, messages: [], nextCursor: null }, sourceRef: null };
  const context = { worktreePath: task.worktreePath ?? task.project?.repoPath ?? null, repoPath: task.project?.repoPath ?? null, sshHost: task.sshHost };
  const reader = transcriptReaders[process.provider];
  const detail = await reader({ ...context, includeInstructions: true }, binding.sessionId, binding.sourceRef, null, TRANSCRIPT_PAGE_LIMIT);
  if (!detail) return unavailable("exact-session-unavailable");
  return { transcript: { status: "available", reason: null, sessionId: detail.sessionId, messages: detail.messages, nextCursor: detail.nextCursor }, sourceRef: binding.sourceRef ?? null };
}

export async function getPaneChatLayout(taskId: string, terminalTabId?: string): Promise<PaneChatLayout> {
  if (typeof taskId !== "string" || !taskId || (terminalTabId !== undefined && (typeof terminalTabId !== "string" || !terminalTabId))) throw inputError("태스크와 터미널 식별자가 올바르지 않습니다.");
  try { return await readPaneChatLayout(taskId, terminalTabId); }
  catch {
    // 이전 좌표를 남기면 바뀐 레이아웃의 다른 pane을 가릴 수 있다.
    return { activePaneId: null, panes: [] };
  }
}

export async function focusPaneChatTerminal(taskId: string, paneId: string): Promise<PaneChatActionResult> {
  if (typeof taskId !== "string" || !taskId || typeof paneId !== "string" || !paneId) throw inputError("태스크와 pane 식별자가 올바르지 않습니다.");
  const surface = await readPaneChatSurface(taskId, paneId, false);
  if (!surface || surface.paneId !== paneId) throw changedTargetError();
  const { task } = surface;
  if (task.sessionType === SessionType.HERDR) {
    await focusHerdrTaskPane(taskId, paneId);
  } else if (task.sessionType === SessionType.TERMINAL) {
    if (!listLocalTerminalTabs(taskId).some((tab) => tab.id === paneId && tab.isActive)) throw changedTargetError();
  } else {
    const command = task.sessionType === SessionType.TMUX
      ? `tmux select-pane -t ${quoteForPosixShell(paneId)}`
      : task.sessionType === SessionType.ZELLIJ
        ? `zellij --session ${quoteForPosixShell(task.sessionName!)} action focus-pane-id ${quoteForPosixShell(paneId)}`
        : null;
    if (!command) throw changedTargetError();
    await execGit(command, task.sshHost, { timeoutMs: PROCESS_PROBE_TIMEOUT_MS });
  }
  return { success: true };
}

const codexModelCatalogs = new Map<string, { readAt: number; models: PaneChatModelOption[] }>();

async function readCodexModelCatalog(sshHost: string | null | undefined): Promise<PaneChatModelOption[]> {
  const hostKey = sshHost ?? "";
  const cached = codexModelCatalogs.get(hostKey);
  if (cached && Date.now() - cached.readAt < CODEX_MODEL_CATALOG_TTL_MS) return cached.models;
  const home = await getHomeDirectory(sshHost);
  const models = parseCodexModelCatalog(JSON.parse(await readTextFile(path.posix.join(home, ".codex", "models_cache.json"), sshHost)));
  codexModelCatalogs.set(hostKey, { readAt: Date.now(), models });
  return models;
}

async function readModelSettings(provider: "claude" | "codex", sourceRef: string, sshHost: string | null | undefined, pending = false): Promise<PaneChatModelSettings> {
  const [records, models] = await Promise.all([
    // 화면을 보는 동안 매초 불리므로, 기록 파일이 바뀌지 않았으면 원격에서 끝부분을 다시 받지 않는다
    pending && !await pathExists(sourceRef, sshHost) ? Promise.resolve([]) : getCachedOrParseTail(sourceRef, () => readJsonLinesTail(sourceRef, MODEL_STATE_TAIL_LINES, sshHost), sshHost),
    provider === "codex" ? readCodexModelCatalog(sshHost) : Promise.resolve([]),
  ]);
  return buildPaneChatModelSettings(provider, provider === "claude" ? readClaudeModelState(records) : readCodexModelState(records), models);
}

export interface PaneChatReading {
  snapshot: PaneChatSnapshot;
  /** 이 데스크탑의 파일 시스템에서 바로 감시할 수 있는 대화 기록 파일. 원격 태스크이거나 기록을 연결하지 못했으면 null이다 */
  localTranscriptFile: string | null;
}

export interface PaneChatReadOptions {
  /** 대화 기록을 연결했어도 CLI 화면을 함께 읽는다. 기록을 연결하지 못했으면 이 값과 상관없이 화면을 읽는다 */
  includeScreen?: boolean;
  /** 같은 pane을 보는 다른 조회가 이 결과를 잠깐 재사용하도록 남긴다. 주기적으로 pane을 읽는 구독 감시만 켠다 */
  isSharedWithOtherViewers?: boolean;
}

interface SharedPaneChatReading {
  taskId: string;
  startedAt: number;
  includesScreen: boolean;
  reading: Promise<PaneChatReading>;
}
/** 구독 감시가 최근에 읽은 결과. 데스크톱 창과 휴대폰이 같은 pane을 볼 때 외부 명령을 두 번 실행하지 않으려고 남긴다 */
const sharedReadings = new Map<string, SharedPaneChatReading>();

function sharedReadingKey(taskId: string, paneId: string | undefined): string {
  // 스냅샷의 targetToken은 요청한 pane 식별자별로 발급되므로, 같은 식별자로 요청한 조회끼리만 공유한다
  return JSON.stringify([taskId, paneId ?? null]);
}

function forgetSharedReadings(taskId: string): void {
  for (const [key, shared] of sharedReadings) {
    if (shared.taskId === taskId) sharedReadings.delete(key);
  }
}

function shareReading(key: string, shared: SharedPaneChatReading): void {
  for (const [candidate, previous] of sharedReadings) {
    if (shared.startedAt - previous.startedAt > SHARED_READING_MAX_AGE_MS) sharedReadings.delete(candidate);
  }
  sharedReadings.set(key, shared);
  shared.reading.catch(() => { if (sharedReadings.get(key) === shared) sharedReadings.delete(key); });
}

/**
 * 구독 감시가 방금 읽은 같은 pane의 결과가 있으면 그것을, 없으면 새로 읽는다.
 * 재사용하는 결과도 읽을 때 대상 확인을 두 번 거친 값이고, 전송은 여전히 `assertTarget`이 다시 확인한다.
 */
export function readRecentPaneChat(taskId: string, paneId?: string, options: Pick<PaneChatReadOptions, "includeScreen"> = {}): Promise<PaneChatReading> {
  const shared = sharedReadings.get(sharedReadingKey(taskId, paneId));
  const isFresh = !!shared && Date.now() - shared.startedAt <= SHARED_READING_MAX_AGE_MS;
  if (shared && isFresh && !mutations.has(taskId) && (shared.includesScreen || !options.includeScreen)) return shared.reading;
  return readPaneChat(taskId, paneId, options);
}

export async function getPaneChatSnapshot(taskId: string, paneId?: string): Promise<PaneChatSnapshot> {
  return (await readRecentPaneChat(taskId, paneId)).snapshot;
}

/** 현재 검증된 pane 대화의 이전 기록만 읽고, 조회 도중 대상이 바뀌면 결과를 버린다. */
export async function getPaneChatTranscriptPage(taskId: string, targetToken: string, cursor: string): Promise<PaneChatSnapshot["transcript"]> {
  if (typeof cursor !== "string" || !/^\d{1,9}$/.test(cursor)) throw inputError("이전 대화 위치가 올바르지 않습니다.");
  const lease = await assertTarget(taskId, targetToken);
  if (!lease.conversation) throw changedTargetError();
  const surface = await readPaneChatSurface(taskId, lease.requestedPaneId, false);
  if (!surface) throw changedTargetError();
  const [sessionId, sourceRef] = JSON.parse(lease.conversation) as [string, string];
  const reader = transcriptReaders[lease.process.provider];
  const detail = await reader({ worktreePath: surface.task.worktreePath ?? surface.task.project?.repoPath ?? null, repoPath: surface.task.project?.repoPath ?? null, sshHost: surface.task.sshHost, includeInstructions: true }, sessionId, sourceRef, cursor, TRANSCRIPT_PAGE_LIMIT);
  await assertTarget(taskId, targetToken);
  if (!detail || detail.sessionId !== sessionId) throw changedTargetError();
  return { status: "available", reason: null, sessionId, messages: detail.messages, nextCursor: detail.nextCursor };
}

/** pane에서 돌고 있는 에이전트 CLI의 이름만 확인한다. 화면과 대화 기록은 읽지 않는다 */
export async function getPaneChatAgentProvider(taskId: string, paneId: string): Promise<PaneChatSnapshot["provider"]> {
  try {
    const surface = await readPaneChatSurface(taskId, paneId, false);
    return surface ? (await inspectProcess(surface))?.provider ?? null : null;
  } catch {
    return null;
  }
}

export function readPaneChat(taskId: string, paneId?: string, options: PaneChatReadOptions = {}): Promise<PaneChatReading> {
  const includeScreen = options.includeScreen === true;
  const reading = readVerifiedPaneChat(taskId, paneId, includeScreen);
  if (options.isSharedWithOtherViewers) shareReading(sharedReadingKey(taskId, paneId), { taskId, startedAt: Date.now(), includesScreen: includeScreen, reading });
  return reading;
}

async function readVerifiedPaneChat(taskId: string, paneId: string | undefined, includeScreen: boolean): Promise<PaneChatReading> {
  const reading = (snapshot: PaneChatSnapshot, localTranscriptFile: string | null = null): PaneChatReading => ({ snapshot, localTranscriptFile });
  if (typeof taskId !== "string" || !taskId || (paneId !== undefined && typeof paneId !== "string")) throw inputError("태스크와 pane 식별자가 올바르지 않습니다.");
  const disabled: PaneChatSnapshot = { taskId, paneId: null, sessionType: null, provider: null, targetToken: null, canSend: false, inputState: "unknown", agentActivity: "unknown", screen: "", reason: "pane-unavailable", transcript: emptyTranscript("exact-session-unavailable"), modelPicker: { supported: false, mode: "native-cli" } };
  let surface: PaneChatSurface | null;
  try { surface = await readPaneChatSurface(taskId, paneId); } catch { return reading(disabled); }
  if (!surface) return reading(disabled);
  const verifiedSurface = surface;
  const snapshot = { ...disabled, paneId: surface.paneId, sessionType: surface.task.sessionType };
  // 대화 기록을 보여 주지 못하는 상태에서는 CLI 화면이 유일하게 보여 줄 내용이라 함께 읽는다
  const readingWithScreen = async (unavailable: PaneChatSnapshot): Promise<PaneChatReading> => {
    try { return reading({ ...unavailable, screen: await readPaneChatScreen(verifiedSurface) }); } catch { return reading(disabled); }
  };
  let process: PaneChatProcess | null;
  try { process = await inspectProcess(surface); } catch { return readingWithScreen({ ...snapshot, reason: "process-inspection-unavailable" }); }
  if (!process) return readingWithScreen({ ...snapshot, reason: "no-foreground-ai-cli" });
  const displayState = await codexDisplayExecution(surface, process);
  const executionIdentity = identityFor(surface, process);
  let binding: Awaited<ReturnType<typeof resolveConversation>>;
  try { binding = await resolveConversation(surface, process); }
  catch (error) { return error instanceof Error && error.name === "PaneChatTargetChangedError" ? reading({ ...disabled, reason: "target-changed" }) : readingWithScreen({ ...snapshot, provider: process.provider, reason: "transcript-read-failed", transcript: emptyTranscript("transcript-read-failed") }); }
  const conversation = conversationIdentity(binding);
  const supportsComposer = process.provider === "claude" || process.provider === "codex";
  const [transcriptRead, modelSettings] = await Promise.all([
    readTranscript(surface, process, binding).catch(() => ({ transcript: emptyTranscript("transcript-read-failed"), sourceRef: null })),
    binding && supportsComposer
      ? readModelSettings(process.provider as "claude" | "codex", binding.sourceRef, surface.task.sshHost, binding.pending).catch(() => undefined)
      : Promise.resolve(undefined),
  ]);
  let { transcript } = transcriptRead;
  const { sourceRef } = transcriptRead;
  if (displayState && !binding) transcript = emptyTranscript(displayState.reason);
  // 화면은 두 번째 확인보다 먼저 읽어, 읽는 사이 대상이 바뀌었으면 그 화면도 함께 버려지게 한다
  let screen = "";
  if (includeScreen || transcript.status !== "available") {
    try { screen = await readPaneChatScreen(surface); } catch { return reading(disabled); }
  }
  try {
    const latest = await readPaneChatSurface(taskId, paneId, !!displayState);
    const latestProcess = latest ? await inspectProcess(latest) : null;
    if (!latest || !latestProcess || identityFor(latest, latestProcess) !== executionIdentity
      || conversationIdentity(await resolveConversation(latest, latestProcess)) !== conversation) return reading({ ...disabled, reason: "target-changed" });
  } catch { return reading({ ...disabled, reason: "target-changed" }); }
  retireChangedLeases(taskId, surface.paneId, executionIdentity, conversation);
  const canSend = transcript.status === "available";
  const targetToken = canSend ? leaseTarget(surface, process, paneId, conversation) : null;
  const conversationKey = binding && canSend ? conversationKeyFor(surface, process, conversation)! : undefined;
  return reading({ ...snapshot, screen, provider: process.provider, targetToken, canSend, ...(conversationKey ? { conversationKey } : {}), inputState: readInputState(surface, process, !!displayState), agentActivity: readAgentActivity(surface, process), reason: canSend ? null : transcript.reason, transcript, modelPicker: { supported: supportsComposer, mode: "native-cli", supportsApi: !!displayState }, ...(modelSettings ? { modelSettings } : {}), composer: { attachments: supportsComposer, skills: supportsComposer, fileMentions: supportsComposer, canSendWhileBusy: !!displayState, maxAttachmentBytes: MAX_ATTACHMENT_BYTES, maxAttachments: MAX_PANE_CHAT_ATTACHMENTS } }, surface.task.sshHost ? null : sourceRef);
}

async function assertTarget(taskId: string, token: string, expectedComposer?: string[]): Promise<TargetLease> {
  const lease = leases.get(token);
  if (!lease || lease.taskId !== taskId || lease.expiresAt < Date.now()) throw changedTargetError();
  let surface: PaneChatSurface | null;
  let process: PaneChatProcess | null;
  try {
    const display = await codexDisplays.get(lease.identity);
    surface = await readPaneChatSurface(taskId, lease.requestedPaneId, !!display);
    process = surface ? await inspectProcess(surface) : null;
  } catch { throw changedTargetError(); }
  if (!surface || !process || identityFor(surface, process) !== lease.identity) {
    leases.delete(token);
    throw changedTargetError();
  }
  try {
    const conversation = conversationIdentity(await resolveConversation(surface, process));
    retireChangedLeases(taskId, surface.paneId, lease.identity, conversation);
    if (conversation !== lease.conversation) throw changedTargetError();
  } catch { throw changedTargetError(); }
  const display = await codexDisplayExecution(surface, process);
  let composerColumns = surface.viewportColumns ?? undefined;
  if (display && expectedComposer && surface.task.sessionType === SessionType.HERDR) {
    // Herdr의 관찰 프레임은 실제 PTY보다 넓게 패딩될 수 있어, 접힌 글을 비교할 때만 원래 PTY 너비를 읽는다.
    composerColumns = undefined;
    const echoed = codexDisplay(surface)?.composerText;
    if (echoed !== undefined && !expectedComposer.some((expected) => isSameComposerText(echoed, expected))) {
      try {
        const tty = (await execGit(`ps -p ${process.pid} -o tty=`, surface.task.sshHost, { timeoutMs: PROCESS_PROBE_TIMEOUT_MS })).trim();
        if (/^(?:pts\/\d+|tty[a-zA-Z0-9]+)$/.test(tty)) {
          const device = quoteForPosixShell(`/dev/${tty}`);
          const size = (await execGit(`stty -F ${device} size 2>/dev/null || stty -f ${device} size 2>/dev/null`, surface.task.sshHost, { timeoutMs: PROCESS_PROBE_TIMEOUT_MS })).trim().match(/^\d+\s+(\d+)$/);
          const columns = Number(size?.[1]);
          if (Number.isInteger(columns) && columns > 2 && columns <= 500) composerColumns = columns;
        }
      } catch { /* 너비가 없으면 줄바꿈을 추정하지 않는다. */ }
    }
  }
  if (display) {
    surface = await sameExecutionSurface(taskId, lease.paneId, lease.identity);
    if (conversationIdentity(await resolveConversation(surface, process)) !== lease.conversation) {
      leases.delete(token);
      throw changedTargetError();
    }
    if (surface.task.sessionType !== SessionType.HERDR) composerColumns = surface.viewportColumns ?? undefined;
  }
  if (leases.get(token) !== lease) throw changedTargetError();
  if (expectedComposer && display) {
    const echoed = codexDisplay(surface)?.composerText;
    const expectsEmpty = expectedComposer.includes("") && expectedComposer.includes("Ask Codex to do anything");
    if ((expectsEmpty && readInputState(surface, process, true) !== "ready") || echoed === undefined || !expectedComposer.some((expected) => isSameComposerText(echoed, expected, composerColumns))) throw inputError("CLI 입력란에서 보낸 내용을 확인하지 못해 제출하지 않았습니다. 터미널에서 확인해 주세요.");
  }
  return lease;
}
async function writeFenced(taskId: string, token: string, bytes: string, onWrite?: () => void, expectedComposer?: string[]): Promise<void> {
  const lease = await assertTarget(taskId, token, expectedComposer);
  if (leases.get(token) !== lease) throw changedTargetError();
  onWrite?.();
  await writeToPane(taskId, lease.paneId, bytes);
}
async function queueMutation(taskId: string, action: () => Promise<void>): Promise<void> {
  const previous = mutations.get(taskId) ?? Promise.resolve();
  const current = previous.catch(() => undefined).then(action);
  mutations.set(taskId, current);
  try { await current; }
  finally {
    if (mutations.get(taskId) === current) mutations.delete(taskId);
    // 입력을 치기 전에 읽은 결과를 재사용하면 방금 보낸 내용이 다음 주기까지 보이지 않는다
    forgetSharedReadings(taskId);
  }
}
/**
 * 같은 동작 안에서 방금 확인한 대상에 pane을 다시 조회하지 않고 쓴다.
 * 그사이 다른 조회가 대상 변경을 알아채 lease를 회수했으면 쓰지 않는다.
 */
async function writeToVerifiedTarget(taskId: string, token: string, lease: TargetLease, bytes: string): Promise<void> {
  if (leases.get(token) !== lease) throw changedTargetError();
  await writeToPane(taskId, lease.paneId, bytes);
}
/** 대상 확인은 pane·프로세스·대화를 다시 조회하는 비싼 일이라, 동작마다 여기서 한 번 하고 그 결과를 동작에 넘긴다 */
async function serializeAction(taskId: string, token: string, action: (lease: TargetLease) => Promise<void>): Promise<PaneChatActionResult> {
  await queueMutation(taskId, async () => { await action(await assertTarget(taskId, token)); });
  return { success: true };
}
/** 이 태스크의 pane에 프롬프트나 명령을 치고 있는 중인지. 그동안의 입력란은 사용자가 아니라 KanVibe가 채운 것이다 */
export function isPaneChatActionInProgress(taskId: string): boolean {
  return mutations.has(taskId);
}
const delay = (milliseconds: number) => new Promise<void>((resolve) => setTimeout(resolve, milliseconds));

async function attachmentOwner(taskId: string, targetToken: string): Promise<PaneChatAttachmentOwner> {
  const lease = await assertTarget(taskId, targetToken);
  if (lease.process.provider !== "claude" && lease.process.provider !== "codex") throw inputError("이 CLI의 첨부 입력은 지원되지 않습니다.");
  const surface = await readPaneChatSurface(taskId, lease.requestedPaneId, false);
  if (!surface || identityFor(surface, lease.process) !== lease.identity) throw changedTargetError();
  return { taskId, targetToken, identity: lease.identity, sshHost: surface.task.sshHost ?? null };
}

export async function stagePaneChatAttachment(taskId: string, targetToken: string, input: PaneChatAttachmentInput): Promise<PaneChatAttachment> {
  const owner = await attachmentOwner(taskId, targetToken);
  const attachment = await storePaneChatAttachment(owner, input);
  try { await assertTarget(taskId, targetToken); }
  catch (error) {
    await discardPaneChatAttachment(taskId, targetToken, attachment.id).catch((cleanupError) => console.warn("[kanvibe] Stale chat attachment cleanup failed", cleanupError));
    throw error;
  }
  return attachment;
}

export async function removePaneChatAttachment(taskId: string, targetToken: string, id: string): Promise<PaneChatActionResult> {
  if (typeof taskId !== "string" || !taskId || typeof targetToken !== "string" || !targetToken) throw inputError("태스크와 CLI 대상이 올바르지 않습니다.");
  await discardPaneChatAttachment(taskId, targetToken, id);
  return { success: true };
}

export async function getPaneChatSuggestions(taskId: string, targetToken: string, kind: PaneChatSuggestionKind, query: string, limit?: number): Promise<PaneChatSuggestions> {
  if (!["skill", "command", "file"].includes(kind) || typeof query !== "string" || query.length > 256 || /[\x00-\x1f\x7f]/.test(query)) throw inputError("검색 종류·검색어가 올바르지 않습니다.");
  if (limit !== undefined && (!Number.isInteger(limit) || limit < 1 || limit > MAX_PANE_CHAT_CATALOG_SUGGESTIONS)) throw inputError("제안 개수 상한이 올바르지 않습니다.");
  const lease = await assertTarget(taskId, targetToken);
  if (lease.process.provider !== "claude" && lease.process.provider !== "codex") return { targetToken, items: [], truncated: false };
  const surface = await readPaneChatSurface(taskId, lease.requestedPaneId, false);
  if (!surface || identityFor(surface, lease.process) !== lease.identity) throw changedTargetError();
  const workspacePath = surface.task.worktreePath ?? surface.task.project?.repoPath;
  if (!workspacePath) throw inputError("현재 CLI의 프로젝트 경로를 확인할 수 없습니다.");
  let cwd = workspacePath;
  if (kind !== "file") {
    try {
      const output = await execGit(`lsof -a -p ${lease.process.pid} -d cwd -Fn`, surface.task.sshHost, { timeoutMs: PROCESS_PROBE_TIMEOUT_MS });
      const actual = output.split("\n").find((line) => line.startsWith("n/"))?.slice(1);
      if (actual) cwd = actual;
    } catch {
      // 프로세스 cwd 조회가 막혀도 task의 검증된 worktree 카탈로그는 사용할 수 있다.
    }
  }
  const result = await readPaneChatSuggestions({ provider: lease.process.provider, cwd, workspacePath, sshHost: surface.task.sshHost, processId: lease.process.pid }, kind, query, limit);
  await assertTarget(taskId, targetToken);
  return { targetToken, ...result };
}

function formatFileMention(filePath: string): string {
  return /[\s@"']/.test(filePath) ? `@"${filePath.replaceAll("\\", "\\\\").replaceAll('"', '\\"')}"` : `@${filePath}`;
}

export function sendPaneChatPrompt(taskId: string, targetToken: string, text: string, attachmentIds: string[] = [], requestId?: string): Promise<PaneChatActionResult> {
  if (typeof text !== "string" || (!text.trim() && attachmentIds?.length === 0) || Buffer.byteLength(text, "utf8") > MAX_PROMPT_BYTES || /[\x00-\x08\x0b-\x1f\x7f]/.test(text) || !Array.isArray(attachmentIds) || attachmentIds.length > MAX_PANE_CHAT_ATTACHMENTS || attachmentIds.some((id) => typeof id !== "string") || (requestId !== undefined && (typeof requestId !== "string" || !requestId || Buffer.byteLength(requestId) > 128))) return Promise.reject(inputError("메시지·첨부 파일·요청 식별자가 올바르지 않습니다."));
  const initialLease = leases.get(targetToken);
  const codexConversation = initialLease?.taskId === taskId && initialLease.process.provider === "codex" ? initialLease.conversationKey : null;
  // Codex operation은 대화에 속한다. lease 갱신·X→Y→X가 이미 수락되거나 불명확한 요청을 새 전송으로 바꾸면 안 된다.
  const receiptOwner = codexConversation ? `conversation:${codexConversation}` : targetToken;
  let fingerprint: string;
  try {
    const attachments = codexConversation && initialLease ? fingerprintPaneChatAttachments({ taskId, targetToken, identity: initialLease.identity, sshHost: (JSON.parse(initialLease.identity) as (string | null)[])[1] ?? null }, attachmentIds) : attachmentIds;
    fingerprint = createHash("sha256").update(JSON.stringify([taskId, text, attachments])).digest("hex");
  } catch (error) { return Promise.reject(error); }
  const previous = requestId ? receipts.get(receiptOwner)?.get(requestId) : null;
  if (previous) return previous.fingerprint === fingerprint ? previous.result : Promise.reject(inputError("동일한 요청 식별자로 다른 메시지를 보낼 수 없습니다."));
  let writeAttempted = false;
  const write = (bytes: string, expectedComposer?: string[]) => writeFenced(taskId, targetToken, bytes, () => { writeAttempted = true; }, expectedComposer);
  const result = serializeAction(taskId, targetToken, async (lease) => {
    const owner = attachmentIds.length ? await attachmentOwner(taskId, targetToken) : null;
    const attachments = owner ? resolvePaneChatAttachments(owner, attachmentIds) : [];
    const files = attachments.filter((entry) => entry.kind === "file").map((entry) => formatFileMention(entry.path));
    let body = [text, ...files].filter(Boolean).join("\n\n");
    const display = await codexDisplays.get(lease.identity);
    const nativeCommand = !attachments.length && /^\/[a-z][a-z0-9-]*(?:\s|$)/i.test(body);
    // Claude 파일 참조 팝업이 전송 Enter를 소비하지 않도록 마지막 토큰을 공백으로 닫는다.
    if (lease.process.provider === "claude" && (files.length || /(?:^|\s)@(?:"(?:[^"\\]|\\.)+"|[^\s"]+)$/.test(body)) && !/\s$/.test(body)) body += " ";
    if (lease.process.provider === "codex" && (!display || nativeCommand) && /(?:^|\s)\$[\w.-]+$/.test(body) && !/\s$/.test(body)) body += " ";
    if (owner) retainPaneChatAttachments(owner, attachmentIds);
    const images = attachments.filter((entry) => entry.kind === "image");
    if (display && lease.conversation && !nativeCommand) {
      const [threadId, sourceRef] = JSON.parse(lease.conversation) as [string, string, number];
      const surface = await readPaneChatSurface(taskId, lease.requestedPaneId, false);
      if (!surface) throw changedTargetError();
      const task = surface.task;
      await sendCodexThreadInput({
        context: { worktreePath: task.worktreePath ?? task.project?.repoPath ?? null, repoPath: task.project?.repoPath ?? null, sshHost: task.sshHost },
        socket: lease.process.codexServerSocket, threadId, sourceRef,
        input: [...images.map((image) => ({ type: "localImage" as const, path: image.path })), ...(body ? [{ type: "text" as const, text: body }] : [])],
        clientUserMessageId: createHash("sha256").update(JSON.stringify([targetToken, requestId ?? randomUUID()])).digest("hex"),
        beforeSend: async () => { await assertTarget(taskId, targetToken); },
        onWrite: () => { writeAttempted = true; },
      });
      return;
    }
    await requireReadyInput(taskId, lease);
    let expectedComposer = ["", "Ask Codex to do anything"];
    for (const image of images) {
      const escaped = image.path.replace(/[^a-zA-Z0-9_./@:\-\u0080-\uffff]/g, "\\$&");
      await write(`\x1b[200~${escaped}\x1b[201~`, expectedComposer);
      expectedComposer = expectCodexComposerTexts("", images.indexOf(image) + 1);
      await delay(IMAGE_ATTACHMENT_SETTLE_MS);
    }
    if (body) await write(`\x1b[200~${body}\x1b[201~`, expectedComposer);
    await delay(PROMPT_SUBMIT_DELAY_MS);
    await write("\r", expectCodexComposerTexts(body, images.length));
  }).catch((error) => {
    if (!writeAttempted && requestId) receipts.get(receiptOwner)?.delete(requestId);
    if (!writeAttempted || error?.name === "PaneChatTargetChangedError") throw error;
    throw Object.assign(new Error("CLI 전송 결과를 확인할 수 없습니다. 터미널에서 확인해 주세요. 같은 요청은 다시 전송하지 않습니다."), { name: "PaneChatDeliveryUncertainError", cause: error });
  });
  if (requestId) {
    const targetReceipts = receipts.get(receiptOwner) ?? new Map();
    targetReceipts.set(requestId, { fingerprint, result });
    receipts.set(receiptOwner, targetReceipts);
  }
  return result;
}
export function interruptPaneChat(taskId: string, targetToken: string): Promise<PaneChatActionResult> {
  return serializeAction(taskId, targetToken, (lease) => writeToVerifiedTarget(taskId, targetToken, lease, "\x03"));
}
/**
 * Enter 직전에 Claude 입력란이 방금 넣은 명령 그대로인지 확인한다.
 * 붙여넣고 기다리는 사이 입력란의 글이 바뀌었거나 선택 화면이 떴으면 Enter가 다른 것을 제출하므로 누르지 않는다.
 */
async function requireClaudeComposerText(taskId: string, lease: TargetLease, expectedText: string): Promise<void> {
  const inputLine = (await readPaneChatSurface(taskId, lease.requestedPaneId))?.inputLine ?? "";
  const markerIndex = inputLine.indexOf("❯");
  const isShowingExpectedText = markerIndex >= 0 && !inputLine.slice(0, markerIndex).trim() && inputLine.slice(markerIndex + 1).trim() === expectedText;
  if (!isShowingExpectedText) throw inputError("CLI 입력란에서 보낸 내용을 확인하지 못해 제출하지 않았습니다. 터미널에서 확인해 주세요.");
}
/**
 * 슬래시 명령을 CLI 입력란에 넣고 Enter로 실행한다.
 * 입력란의 글을 화면에서 읽을 수 있는 Codex에는 글자마다 에코를 확인하며 치고, 그 밖의 CLI에는 한 번에 붙여넣는다.
 * 어느 쪽이든 Enter는 기다린 뒤 대상을 다시 확인하고 보낸다. Claude는 입력란에 넣은 명령이 그대로 있는지도 마지막에 확인한다.
 */
async function submitPaneChatCommand(taskId: string, targetToken: string, lease: TargetLease, command: string): Promise<void> {
  if (await codexDisplays.get(lease.identity)) {
    let echoed = "";
    for (const character of command) {
      await writeFenced(taskId, targetToken, character, undefined, echoed ? [echoed] : ["", "Ask Codex to do anything"]);
      echoed += character;
      await delay(PICKER_CHARACTER_DELAY_MS);
    }
  } else {
    await writeToVerifiedTarget(taskId, targetToken, lease, `\x1b[200~${command}\x1b[201~`);
  }
  await delay(PROMPT_SUBMIT_DELAY_MS);
  const verifiedLease = await assertTarget(taskId, targetToken, [command]);
  if (verifiedLease.process.provider === "claude") await requireClaudeComposerText(taskId, verifiedLease, command);
  await writeToVerifiedTarget(taskId, targetToken, verifiedLease, "\r");
}
export function openPaneChatModelPicker(taskId: string, targetToken: string): Promise<PaneChatActionResult> {
  return serializeAction(taskId, targetToken, async (lease) => {
    if (lease.process.provider !== "claude" && lease.process.provider !== "codex") throw inputError("이 CLI의 모델 선택기는 지원되지 않습니다.");
    await requireReadyInput(taskId, lease);
    await submitPaneChatCommand(taskId, targetToken, lease, "/model");
  });
}

/**
 * 사용자가 "대화 ID 표시"를 눌렀을 때만 Codex에 `/statusline thread-id`를 붙여넣고 실행해, footer에 대화 ID를 띄운다.
 *
 * 공유 데몬에서 도는 Codex는 화면 밖에서 대화를 알려 주지 않으므로 footer가 유일한 식별 신호다. 표시 배치를 실제로 확인한 버전에서,
 * Codex 고유의 빈 입력란이 확인될 때만 입력한다. 붙여넣기 전후와 Enter 직전에 같은 pane·같은 CLI 프로세스인지 다시 확인하고,
 * 입력란에 명령이 그대로 보이지 않으면 Enter를 보내지 않는다. 다른 pane 입력과 겹치지 않도록 태스크의 입력 큐에서 실행한다.
 */
export async function revealPaneChatCodexThread(taskId: string, paneId?: string): Promise<PaneChatCodexThreadReveal> {
  if (typeof taskId !== "string" || !taskId || (paneId !== undefined && (typeof paneId !== "string" || !paneId))) throw inputError("태스크와 pane 식별자가 올바르지 않습니다.");
  let outcome: PaneChatCodexThreadReveal["outcome"] = "thread-not-found";
  await queueMutation(taskId, async () => {
    const surface = await readPaneChatSurface(taskId, paneId);
    const process = surface ? await inspectProcess(surface) : null;
    if (!surface || process?.provider !== "codex") throw changedTargetError();
    if (!await codexDisplayExecution(surface, process)) { outcome = "unsupported-version"; return; }
    if (codexDisplay(surface)?.threadId) { outcome = "revealed"; return; }
    const identity = identityFor(surface, process);
    const readSurface = () => sameExecutionSurface(taskId, surface.paneId, identity);
    if (readInputState(await readSurface(), process, true) !== "ready") { outcome = "input-not-empty"; return; }
    await writeToPane(taskId, surface.paneId, `\x1b[200~${CODEX_THREAD_FOOTER_COMMAND}\x1b[201~`);
    if (!await waitForCodexScreen(readSurface, (current) => isSameComposerText(codexDisplay(current)?.composerText ?? "", CODEX_THREAD_FOOTER_COMMAND, current.viewportColumns ?? undefined))) {
      throw inputError("Codex 입력란에서 붙여넣은 명령을 확인하지 못해 실행하지 않았습니다. 터미널에서 입력란을 확인해 주세요.");
    }
    await delay(PROMPT_SUBMIT_DELAY_MS);
    const beforeSubmit = await readSurface();
    if (!isSameComposerText(codexDisplay(beforeSubmit)?.composerText ?? "", CODEX_THREAD_FOOTER_COMMAND, beforeSubmit.viewportColumns ?? undefined)) {
      throw inputError("Codex 입력란이 바뀌어 대화 ID 표시 명령을 실행하지 않았습니다. 터미널에서 입력란을 확인해 주세요.");
    }
    await writeToPane(taskId, surface.paneId, "\r");
    if (await waitForCodexScreen(readSurface, (current) => !!codexDisplay(current)?.threadId)) outcome = "revealed";
  });
  return { outcome };
}
async function waitForCodexScreen(readSurface: () => Promise<PaneChatSurface>, isExpected: (surface: PaneChatSurface) => boolean): Promise<boolean> {
  for (let attempt = 0; attempt < CODEX_FOOTER_READ_ATTEMPTS; attempt++) {
    if (isExpected(await readSurface())) return true;
    await delay(CODEX_FOOTER_READ_INTERVAL_MS);
  }
  return false;
}

async function codexSettingsTarget(taskId: string, targetToken: string) {
  const lease = await assertTarget(taskId, targetToken);
  const surface = await readPaneChatSurface(taskId, lease.requestedPaneId, false);
  if (!surface || lease.process.provider !== "codex" || !lease.conversation || !await codexDisplayExecution(surface, lease.process)) throw inputError("이 CLI는 모델 설정 API를 지원하지 않습니다.");
  const [threadId, sourceRef] = JSON.parse(lease.conversation) as [string, string];
  return { context: { worktreePath: surface.task.worktreePath ?? surface.task.project?.repoPath ?? null, repoPath: surface.task.project?.repoPath ?? null, sshHost: surface.task.sshHost }, socket: lease.process.codexServerSocket, threadId, sourceRef, beforeWrite: async () => { await assertTarget(taskId, targetToken); } };
}

export async function getPaneChatModelSettings(taskId: string, targetToken: string): Promise<PaneChatModelSettings> {
  const lease = await assertTarget(taskId, targetToken);
  const { provider } = lease.process;
  if (!lease.conversation || (provider !== "claude" && provider !== "codex")) throw inputError("이 CLI의 모델 설정은 지원되지 않습니다.");
  const [, sourceRef] = JSON.parse(lease.conversation) as [string, string];
  const isCodexDisplay = provider === "codex" && !!await codexDisplays.get(lease.identity);
  // 대화 ID 표시 대상의 대화는 화면 하단에서만 읽힌다. 화면 없이 다시 확인하면 대화가 사라진 것으로 판정돼 연결이 끊긴다.
  const surface = await readPaneChatSurface(taskId, lease.requestedPaneId, isCodexDisplay);
  if (!surface) throw changedTargetError();
  const binding = await resolveConversation(surface, lease.process);
  if (conversationIdentity(binding) !== lease.conversation) throw changedTargetError();
  const settings = isCodexDisplay
    ? await readCodexThreadSettings(await codexSettingsTarget(taskId, targetToken))
    : await readModelSettings(provider, sourceRef, surface.task.sshHost, binding?.pending);
  await assertTarget(taskId, targetToken);
  return settings;
}
/**
 * 모델·effort·빠른 모드를 CLI의 슬래시 명령으로 바꾼다.
 *
 * 명령은 지금 읽은 선택지 목록으로만 만든다. Claude가 모델 전환을 되물으면 CLI 화면에 그 확인이 뜨고, 답은 사용자가 한다.
 */
export function changePaneChatModelSetting(taskId: string, targetToken: string, requestedChange: unknown): Promise<PaneChatActionResult> {
  const change = parsePaneChatModelSettingChange(requestedChange);
  if (!change) return Promise.reject(inputError("모델 설정 변경 형식이 올바르지 않습니다."));
  return serializeAction(taskId, targetToken, async (lease) => {
    const { provider } = lease.process;
    if (provider !== "claude" && provider !== "codex") throw inputError("이 CLI의 모델 설정은 지원되지 않습니다.");
    if (provider === "codex" && await codexDisplays.get(lease.identity)) {
      await updateCodexThreadSettings(await codexSettingsTarget(taskId, targetToken), change);
      return;
    }
    await requireReadyInput(taskId, lease);
    const { modelSettings } = (await readPaneChat(taskId, lease.requestedPaneId)).snapshot;
    const command = modelSettings ? buildPaneChatModelCommand(provider, modelSettings, change) : null;
    if (!command) throw inputError("지금은 이 설정을 바꿀 수 없습니다. 터미널에서 현재 상태를 확인해 주세요.");
    await submitPaneChatCommand(taskId, targetToken, lease, command);
  });
}
export function sendPaneChatKey(taskId: string, targetToken: string, key: PaneChatKey): Promise<PaneChatActionResult> {
  if (!Object.hasOwn(KEY_BYTES, key)) return Promise.reject(inputError("허용되지 않는 선택기 키입니다."));
  return serializeAction(taskId, targetToken, (lease) => writeToVerifiedTarget(taskId, targetToken, lease, KEY_BYTES[key]));
}
