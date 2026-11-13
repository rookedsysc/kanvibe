import { createHash, randomUUID } from "node:crypto";
import path from "node:path";
import { SessionType } from "@/entities/KanbanTask";
import { execGit } from "@/lib/gitOperations";
import { focusHerdrTaskPane, listLocalTerminalTabs } from "@/lib/terminal";
import { quoteForPosixShell } from "@/lib/worktree";
import { getHomeDirectory } from "@/lib/hostFileAccess";
import { MAX_ATTACHMENT_BYTES } from "@/lib/attachmentContent";
import { readJsonLinesHead } from "@/lib/aiSessions/shared";
import { readClaudeSessionDetail } from "@/lib/aiSessions/readClaudeSessions";
import { readCodexSessionDetail } from "@/lib/aiSessions/readCodexSessions";
import { findPaneChatProcess, type PaneChatProcess } from "@/lib/aiSessions/paneChatProcess";
import { findPaneChatSessionBinding } from "@/lib/aiSessions/paneChatSessionBindings";
import { MAX_PANE_CHAT_ATTACHMENTS, type PaneChatActionResult, type PaneChatAttachment, type PaneChatAttachmentInput, type PaneChatKey, type PaneChatLayout, type PaneChatSnapshot, type PaneChatSuggestionKind, type PaneChatSuggestions } from "@/desktop/shared/paneChat";
import { readPaneChatLayout, readPaneChatSurface, type PaneChatSurface } from "./paneChatSurface";
import { discardPaneChatAttachment, resolvePaneChatAttachments, retainPaneChatAttachments, storePaneChatAttachment, type PaneChatAttachmentOwner } from "./paneChatAttachmentService";
import { readPaneChatSuggestions } from "./paneChatComposerCatalog";
import { writeToPane } from "./mobileBridgeService";

const PROCESS_TABLE_COMMAND = "ps -wwaxo pid=,ppid=,pgid=,tpgid=,tty=,lstart=,ucomm=,args=";
const PROCESS_PROBE_TIMEOUT_MS = 5_000;
const MAX_PROMPT_BYTES = 64 * 1024;
const MAX_TARGET_LEASES = 256;
const TARGET_LEASE_TTL_MS = 10 * 60 * 1_000;
const TRANSCRIPT_PAGE_LIMIT = 100;
// Orca CLI bridge와 같은 간격으로 붙여넣기 처리가 끝난 뒤 Enter를 보내고 선택기 명령은 한 글자씩 친다.
const PROMPT_SUBMIT_DELAY_MS = 500;
const PICKER_CHARACTER_DELAY_MS = 40;
const IMAGE_ATTACHMENT_SETTLE_MS = 300;
const KEY_BYTES: Record<PaneChatKey, string> = { up: "\x1b[A", down: "\x1b[B", left: "\x1b[D", right: "\x1b[C", enter: "\r", escape: "\x1b" };

interface TargetLease {
  taskId: string;
  requestedPaneId?: string;
  paneId: string;
  identity: string;
  process: PaneChatProcess;
  expiresAt: number;
}
const leases = new Map<string, TargetLease>();
const mutations = new Map<string, Promise<unknown>>();
const receipts = new Map<string, Map<string, { fingerprint: string; result: Promise<PaneChatActionResult> }>>();

function inputError(message: string): Error {
  return Object.assign(new Error(message), { name: "PaneChatInputError" });
}
function changedTargetError(): Error {
  return Object.assign(new Error("CLI 대상이 변경되었습니다. 현재 pane을 다시 확인해 주세요."), { name: "PaneChatTargetChangedError" });
}
function identityFor(surface: PaneChatSurface, process: PaneChatProcess): string {
  return JSON.stringify([surface.task.id, surface.task.sshHost, surface.task.sessionType, surface.task.sessionName, surface.paneId, surface.rootPid, process.provider, process.pid, process.processStartTime]);
}
async function inspectProcess(surface: PaneChatSurface): Promise<PaneChatProcess | null> {
  if (!surface.rootPid) return null;
  return findPaneChatProcess(await execGit(PROCESS_TABLE_COMMAND, surface.task.sshHost, { timeoutMs: PROCESS_PROBE_TIMEOUT_MS }), surface.rootPid);
}
function leaseTarget(surface: PaneChatSurface, process: PaneChatProcess, requestedPaneId?: string): string {
  const now = Date.now();
  const identity = identityFor(surface, process);
  for (const [token, lease] of leases) {
    if (lease.expiresAt < now) { leases.delete(token); receipts.delete(token); }
    else if (lease.identity === identity && lease.requestedPaneId === requestedPaneId) {
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
  leases.set(token, { taskId: surface.task.id, requestedPaneId, paneId: surface.paneId, identity, process, expiresAt: now + TARGET_LEASE_TTL_MS });
  return token;
}
function readInputState(surface: PaneChatSurface, process: PaneChatProcess): PaneChatSnapshot["inputState"] {
  if (surface.inputLine === null || surface.cursorColumn === null) return "unknown";
  const marker = process.provider === "claude" ? "❯" : process.provider === "codex" ? "›" : null;
  if (!marker) return "unknown";
  const line = surface.inputLine;
  const start = line.indexOf(marker);
  if (start < 0 || line.slice(0, start).trim()) return "unknown";
  const value = line.slice(start + marker.length).trim();
  const isPlaceholder = process.provider === "codex" ? value === "Ask Codex to do anything" : /^Try\s+/.test(value);
  return surface.cursorColumn === start + marker.length + 1 && (!value || isPlaceholder) ? "ready" : "occupied";
}
async function requireReadyInput(taskId: string, token: string): Promise<void> {
  const lease = await assertTarget(taskId, token);
  const surface = await readPaneChatSurface(taskId, lease.requestedPaneId);
  if (!surface || readInputState(surface, lease.process) !== "ready") throw inputError("CLI 입력란이 비어 있지 않거나 선택 화면이 열려 있습니다. 터미널에서 현재 상태를 확인해 주세요.");
}

function emptyTranscript(reason: string): PaneChatSnapshot["transcript"] {
  return { status: "unavailable", reason, sessionId: null, messages: [], nextCursor: null };
}
async function readTranscript(surface: PaneChatSurface, process: PaneChatProcess): Promise<PaneChatSnapshot["transcript"]> {
  if (process.provider !== "claude" && process.provider !== "codex") return emptyTranscript("provider-transcript-unavailable");
  const { task } = surface;
  let binding = await findPaneChatSessionBinding({ taskId: task.id, sshHost: task.sshHost, provider: process.provider, providerPid: process.pid, processStartTime: process.processStartTime });
  if (!binding) {
    try {
      const output = await execGit(`lsof -a -p ${process.pid} -Fn`, task.sshHost, { timeoutMs: PROCESS_PROBE_TIMEOUT_MS });
      const home = await getHomeDirectory(task.sshHost);
      const root = process.provider === "claude" ? path.posix.join(home, ".claude", "projects") : path.posix.join(home, ".codex", "sessions");
      const candidates = [...new Set(output.split("\n").filter((line) => line.startsWith(`n${root}/`) && line.endsWith(".jsonl")).map((line) => line.slice(1)))];
      const exactBindings: { sessionId: string; sourceRef: string }[] = [];
      for (const sourceRef of candidates) {
        const records = await readJsonLinesHead(sourceRef, TRANSCRIPT_PAGE_LIMIT, task.sshHost) as { sessionId?: string; type?: string; payload?: { id?: string } }[];
        const sessionId = process.provider === "claude" ? records.find((record) => typeof record.sessionId === "string")?.sessionId : records.find((record) => record.type === "session_meta")?.payload?.id;
        if (sessionId) exactBindings.push({ sessionId, sourceRef });
      }
      if (exactBindings.length === 1) binding = exactBindings[0];
    } catch {
      // CLI가 쓰기 사이에 JSONL을 닫을 수 있다. 열린 파일이 없으면 다른 기록을 연결하지 않는다.
    }
  }
  if (!binding) return emptyTranscript("exact-session-unavailable");
  const context = { worktreePath: task.worktreePath ?? task.project?.repoPath ?? null, repoPath: task.project?.repoPath ?? null, sshHost: task.sshHost };
  const reader = process.provider === "claude" ? readClaudeSessionDetail : readCodexSessionDetail;
  const detail = await reader(context, binding.sessionId, binding.sourceRef, null, TRANSCRIPT_PAGE_LIMIT);
  if (!detail) return emptyTranscript("exact-session-unavailable");
  return { status: "available", reason: null, sessionId: detail.sessionId, messages: detail.messages, nextCursor: detail.nextCursor };
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

export async function getPaneChatSnapshot(taskId: string, paneId?: string): Promise<PaneChatSnapshot> {
  if (typeof taskId !== "string" || !taskId || (paneId !== undefined && typeof paneId !== "string")) throw inputError("태스크와 pane 식별자가 올바르지 않습니다.");
  const disabled: PaneChatSnapshot = { taskId, paneId: null, sessionType: null, provider: null, targetToken: null, canSend: false, inputState: "unknown", screen: "", reason: "pane-unavailable", transcript: emptyTranscript("exact-session-unavailable"), modelPicker: { supported: false, mode: "native-cli" } };
  let surface: PaneChatSurface | null;
  try { surface = await readPaneChatSurface(taskId, paneId); } catch { return disabled; }
  if (!surface) return disabled;
  const snapshot = { ...disabled, paneId: surface.paneId, sessionType: surface.task.sessionType, screen: surface.screen };
  let process: PaneChatProcess | null;
  try { process = await inspectProcess(surface); } catch { return { ...snapshot, reason: "process-inspection-unavailable" }; }
  if (!process) return { ...snapshot, reason: "no-foreground-ai-cli" };
  const targetToken = leaseTarget(surface, process, paneId);
  let transcript = emptyTranscript("exact-session-unavailable");
  try { transcript = await readTranscript(surface, process); } catch { /* 기록 조회 실패가 별도로 검증한 CLI 입력까지 막지는 않는다. */ }
  const supportsComposer = process.provider === "claude" || process.provider === "codex";
  return { ...snapshot, provider: process.provider, targetToken, canSend: true, inputState: readInputState(surface, process), reason: null, transcript, modelPicker: { supported: supportsComposer, mode: "native-cli" }, composer: { attachments: supportsComposer, skills: supportsComposer, fileMentions: supportsComposer, maxAttachmentBytes: MAX_ATTACHMENT_BYTES, maxAttachments: MAX_PANE_CHAT_ATTACHMENTS } };
}

async function assertTarget(taskId: string, token: string): Promise<TargetLease> {
  const lease = leases.get(token);
  if (!lease || lease.taskId !== taskId || lease.expiresAt < Date.now()) throw changedTargetError();
  let surface: PaneChatSurface | null;
  let process: PaneChatProcess | null;
  try {
    surface = await readPaneChatSurface(taskId, lease.requestedPaneId, false);
    process = surface ? await inspectProcess(surface) : null;
  } catch { throw changedTargetError(); }
  if (!surface || !process || identityFor(surface, process) !== lease.identity) throw changedTargetError();
  return lease;
}
async function writeFenced(taskId: string, token: string, bytes: string, onWrite?: () => void): Promise<void> {
  const lease = await assertTarget(taskId, token);
  onWrite?.();
  await writeToPane(taskId, lease.paneId, bytes);
}
async function serializeAction(taskId: string, token: string, action: () => Promise<void>): Promise<PaneChatActionResult> {
  const previous = mutations.get(taskId) ?? Promise.resolve();
  const current = previous.catch(() => undefined).then(async () => { await assertTarget(taskId, token); await action(); });
  mutations.set(taskId, current);
  try { await current; return { success: true }; }
  finally { if (mutations.get(taskId) === current) mutations.delete(taskId); }
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

export async function getPaneChatSuggestions(taskId: string, targetToken: string, kind: PaneChatSuggestionKind, query: string): Promise<PaneChatSuggestions> {
  if (!["skill", "command", "file"].includes(kind) || typeof query !== "string" || query.length > 256 || /[\x00-\x1f\x7f]/.test(query)) throw inputError("검색 종류·검색어가 올바르지 않습니다.");
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
  const result = await readPaneChatSuggestions({ provider: lease.process.provider, cwd, workspacePath, sshHost: surface.task.sshHost, processId: lease.process.pid }, kind, query);
  await assertTarget(taskId, targetToken);
  return { targetToken, ...result };
}

function formatFileMention(filePath: string): string {
  return /[\s@"']/.test(filePath) ? `@"${filePath.replaceAll("\\", "\\\\").replaceAll('"', '\\"')}"` : `@${filePath}`;
}

export function sendPaneChatPrompt(taskId: string, targetToken: string, text: string, attachmentIds: string[] = [], requestId?: string): Promise<PaneChatActionResult> {
  if (typeof text !== "string" || (!text.trim() && attachmentIds?.length === 0) || Buffer.byteLength(text, "utf8") > MAX_PROMPT_BYTES || /[\x00-\x08\x0b-\x1f\x7f]/.test(text) || !Array.isArray(attachmentIds) || attachmentIds.length > MAX_PANE_CHAT_ATTACHMENTS || attachmentIds.some((id) => typeof id !== "string") || (requestId !== undefined && (typeof requestId !== "string" || !requestId || Buffer.byteLength(requestId) > 128))) return Promise.reject(inputError("메시지·첨부 파일·요청 식별자가 올바르지 않습니다."));
  const fingerprint = createHash("sha256").update(JSON.stringify([taskId, text, attachmentIds])).digest("hex");
  const previous = requestId ? receipts.get(targetToken)?.get(requestId) : null;
  if (previous) return previous.fingerprint === fingerprint ? previous.result : Promise.reject(inputError("동일한 요청 식별자로 다른 메시지를 보낼 수 없습니다."));
  let writeAttempted = false;
  const write = (bytes: string) => writeFenced(taskId, targetToken, bytes, () => { writeAttempted = true; });
  const result = serializeAction(taskId, targetToken, async () => {
    await requireReadyInput(taskId, targetToken);
    const lease = await assertTarget(taskId, targetToken);
    const owner = attachmentIds.length ? await attachmentOwner(taskId, targetToken) : null;
    const attachments = owner ? resolvePaneChatAttachments(owner, attachmentIds) : [];
    const files = attachments.filter((entry) => entry.kind === "file").map((entry) => formatFileMention(entry.path));
    let body = [text, ...files].filter(Boolean).join("\n\n");
    if (lease.process.provider === "codex" && /(?:^|\s)\$[\w.-]+$/.test(body) && !/\s$/.test(body)) body += " ";
    if (owner) retainPaneChatAttachments(owner, attachmentIds);
    const images = attachments.filter((entry) => entry.kind === "image");
    for (const image of images) {
      const escaped = image.path.replace(/[^a-zA-Z0-9_./@:\-\u0080-\uffff]/g, "\\$&");
      await write(`\x1b[200~${escaped}\x1b[201~`);
    }
    if (images.length) await delay(IMAGE_ATTACHMENT_SETTLE_MS);
    if (body) await write(`\x1b[200~${body}\x1b[201~`);
    await delay(PROMPT_SUBMIT_DELAY_MS);
    await write("\r");
  }).catch((error) => {
    if (!writeAttempted && requestId) receipts.get(targetToken)?.delete(requestId);
    if (!writeAttempted || error?.name === "PaneChatTargetChangedError") throw error;
    throw Object.assign(new Error("CLI 전송 결과를 확인할 수 없습니다. 터미널에서 확인해 주세요. 같은 요청은 다시 전송하지 않습니다."), { name: "PaneChatDeliveryUncertainError", cause: error });
  });
  if (requestId) {
    const targetReceipts = receipts.get(targetToken) ?? new Map();
    targetReceipts.set(requestId, { fingerprint, result });
    receipts.set(targetToken, targetReceipts);
  }
  return result;
}
export function interruptPaneChat(taskId: string, targetToken: string): Promise<PaneChatActionResult> {
  return serializeAction(taskId, targetToken, () => writeFenced(taskId, targetToken, "\x03"));
}
export function openPaneChatModelPicker(taskId: string, targetToken: string): Promise<PaneChatActionResult> {
  return serializeAction(taskId, targetToken, async () => {
    const lease = await assertTarget(taskId, targetToken);
    if (lease.process.provider !== "claude" && lease.process.provider !== "codex") throw inputError("이 CLI의 모델 선택기는 지원되지 않습니다.");
    await requireReadyInput(taskId, targetToken);
    for (const character of "/model") { await writeFenced(taskId, targetToken, character); await delay(PICKER_CHARACTER_DELAY_MS); }
    await delay(PROMPT_SUBMIT_DELAY_MS);
    await writeFenced(taskId, targetToken, "\r");
  });
}
export function sendPaneChatKey(taskId: string, targetToken: string, key: PaneChatKey): Promise<PaneChatActionResult> {
  if (!Object.hasOwn(KEY_BYTES, key)) return Promise.reject(inputError("허용되지 않는 선택기 키입니다."));
  return serializeAction(taskId, targetToken, () => writeFenced(taskId, targetToken, KEY_BYTES[key]));
}
