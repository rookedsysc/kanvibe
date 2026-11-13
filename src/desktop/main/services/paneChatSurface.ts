import { Terminal as HeadlessTerminal } from "@xterm/headless";
import { stripVTControlCharacters } from "node:util";
import { SessionType, type KanbanTask } from "@/entities/KanbanTask";
import { execGit } from "@/lib/gitOperations";
import { listLocalTerminalTabs, readLocalTerminalTabSnapshot, readTaskTerminalAttachment, getHerdrTaskActivePaneId, getHerdrTaskSurfaceBounds } from "@/lib/terminal";
import { listHerdrMirrorPanes, runHerdrCommand } from "@/lib/herdr";
import { buildTmuxCapturePaneCommand, buildTmuxListMirrorPanesCommand, buildZellijDumpPaneCommand, buildZellijListMirrorPanesCommand, buildZellijReadViewportCommand, parseTmuxMirrorPaneList, parseZellijMirrorPaneList } from "@/lib/paneMirror";
import { quoteForPosixShell } from "@/lib/worktree";
import type { PaneChatBounds, PaneChatLayout, PaneChatLayoutPane } from "@/desktop/shared/paneChat";
import { getTaskById } from "./kanbanService";
import { readHerdrPaneFrameForChat } from "./mobileBridgeService";

const PANE_CHAT_PROBE_TIMEOUT_MS = 5_000;

export interface PaneChatSurface {
  task: KanbanTask;
  paneId: string;
  rootPid: number | null;
  screen: string;
  inputLine: string | null;
  cursorColumn: number | null;
}

export async function readPaneChatSurface(taskId: string, requestedPaneId?: string, capture = true): Promise<PaneChatSurface | null> {
  const task = await getTaskById(taskId);
  if (!task?.sessionType || !task.sessionName) return null;
  const run = (command: string) => execGit(command, task.sshHost, { timeoutMs: PANE_CHAT_PROBE_TIMEOUT_MS });

  if (task.sessionType === SessionType.TERMINAL) {
    // 원격 plain PTY의 로컬 PID는 ssh이므로 원격 CLI의 신원을 보장할 수 없다.
    if (task.sshHost) return null;
    const tabs = listLocalTerminalTabs(taskId);
    const tab = requestedPaneId ? tabs.find((entry) => entry.id === requestedPaneId) : tabs.find((entry) => entry.isActive);
    if (!tab) return null;
    const snapshot = readLocalTerminalTabSnapshot(taskId, tab.id);
    return snapshot ? { task, paneId: tab.id, rootPid: snapshot.pid, screen: capture ? stripVTControlCharacters(snapshot.screen) : "", inputLine: snapshot.inputLine, cursorColumn: snapshot.cursorColumn } : null;
  }

  if (task.sessionType === SessionType.TMUX) {
    let selectedPaneId = requestedPaneId;
    if (!selectedPaneId) {
      const attachment = readTaskTerminalAttachment(taskId);
      if (task.sshHost || !attachment || attachment.sessionType !== task.sessionType || attachment.sessionName !== task.sessionName) return null;
      const clients = await run("tmux list-clients -F '#{client_pid}\t#{session_name}\t#{session_id}\t#{session_group}\t#{pane_id}'");
      const client = clients.split("\n").map((line) => line.split("\t")).find((entry) => Number(entry[0]) === attachment.pid);
      if (!client) return null;
      const [canonicalId, canonicalGroup] = (await run(`tmux display-message -p -t ${quoteForPosixShell(task.sessionName)} '#{session_id}\t#{session_group}'`)).trimEnd().split("\t");
      // Grouped session은 창을 공유해도 활성 창은 다르므로 실제 attach된 client의 pane만 고른다.
      if (client[2] !== canonicalId && (!canonicalGroup || client[3] !== canonicalGroup)) return null;
      selectedPaneId = client[4];
      if (!selectedPaneId) return null;
    }
    const format = "#{pane_id}\t#{pane_pid}\t#{window_active}\t#{pane_active}";
    const output = await run(`tmux list-panes -s -t ${quoteForPosixShell(task.sessionName)} -F ${quoteForPosixShell(format)}`);
    const panes = output.trim().split("\n").map((line) => line.split("\t"));
    const pane = panes.find((entry) => entry[0] === selectedPaneId);
    if (!pane || !Number.isSafeInteger(Number(pane[1])) || Number(pane[1]) < 1) return null;
    let inputLine: string | null = null;
    let cursorColumn: number | null = null;
    if (capture) {
      const cursorAndScreen = await run(`tmux display-message -p -t ${quoteForPosixShell(pane[0])} '#{cursor_x}\t#{cursor_y}' && tmux capture-pane -p -t ${quoteForPosixShell(pane[0])}`);
      const [cursor, ...lines] = cursorAndScreen.split("\n");
      const [column, row] = cursor.split("\t").map(Number);
      if (Number.isInteger(column) && Number.isInteger(row)) { cursorColumn = column; inputLine = lines[row] ?? null; }
    }
    return { task, paneId: pane[0], rootPid: Number(pane[1]), screen: capture ? stripVTControlCharacters(await run(buildTmuxCapturePaneCommand(pane[0]))) : "", inputLine, cursorColumn };
  }

  if (task.sessionType === SessionType.HERDR) {
    const panes = await listHerdrMirrorPanes(task.sessionName, task.sshHost);
    const paneId = requestedPaneId ?? getHerdrTaskActivePaneId(taskId);
    if (!paneId) return null;
    if (!panes.some((entry) => entry.id === paneId)) return null;
    const { process_info: info } = await runHerdrCommand<{ process_info: { shell_pid?: number; foreground_processes?: { pid?: number }[] } }>(task.sessionName, ["pane", "process-info", "--pane", paneId], task.sshHost);
    const pid = info.shell_pid ?? info.foreground_processes?.[0]?.pid;
    const surface: PaneChatSurface = { task, paneId, rootPid: Number.isSafeInteger(pid) && pid! > 0 ? pid! : null, inputLine: null, cursorColumn: null, screen: "" };
    if (!capture) return surface;
    const frame = await readHerdrPaneFrameForChat(taskId, paneId);
    const terminal = new HeadlessTerminal({ cols: frame.width, rows: frame.height, allowProposedApi: true });
    try {
      await new Promise<void>((resolve) => terminal.write(frame.output, resolve));
      const buffer = terminal.buffer.active;
      return { ...surface, inputLine: buffer.getLine(buffer.baseY + buffer.cursorY)?.translateToString(true) ?? null, cursorColumn: buffer.cursorX,
        screen: Array.from({ length: terminal.rows }, (_, row) => buffer.getLine(buffer.baseY + row)?.translateToString(true) ?? "").join("\n").trimEnd() };
    } finally { terminal.dispose(); }
  }

  if (task.sessionType === SessionType.ZELLIJ) {
    const response = JSON.parse(await run(requestedPaneId ? buildZellijListMirrorPanesCommand(task.sessionName) : buildZellijReadViewportCommand(task.sessionName)));
    const panes: unknown = requestedPaneId ? response : response?.[0];
    if (!Array.isArray(panes)) return null;
    const activeTab = !requestedPaneId && Array.isArray(response?.[1]) ? response[1].find((entry: { active?: boolean }) => entry?.active === true) : null;
    const pane = panes.find((entry) => entry?.is_plugin === false && (requestedPaneId ? `terminal_${entry.id}` === requestedPaneId : activeTab && entry.tab_id === activeTab.tab_id && entry.is_focused === true));
    if (!pane) return null;
    const paneId = `terminal_${pane.id}`;
    // PID를 제공하지 않는 Zellij 버전은 정확한 화면만 반환하고 입력은 허용하지 않는다.
    const pid = pane.terminal_pid ?? pane.pid;
    return { task, paneId, rootPid: Number.isSafeInteger(pid) && pid > 0 ? pid : null, inputLine: null, cursorColumn: null, screen: capture ? stripVTControlCharacters(await run(buildZellijDumpPaneCommand(task.sessionName, paneId))) : "" };
  }
  return null;
}

function hasValidPaneBounds(bounds: PaneChatBounds): boolean {
  return Object.values(bounds).every(Number.isInteger) && bounds.left >= 0 && bounds.top >= 0 && bounds.width > 0 && bounds.height > 0;
}

export async function readPaneChatLayout(taskId: string, terminalTabId?: string): Promise<PaneChatLayout> {
  const unavailable: PaneChatLayout = { activePaneId: null, panes: [] };
  const surface = await readPaneChatSurface(taskId, terminalTabId, false);
  if (!surface) return unavailable;
  const { task, paneId } = surface;
  if (task.sessionType === SessionType.TERMINAL) return { activePaneId: paneId, panes: [{ id: paneId, tabId: paneId, bounds: null }] };
  const run = (command: string) => execGit(command, task.sshHost, { timeoutMs: PANE_CHAT_PROBE_TIMEOUT_MS });
  let panes: PaneChatLayoutPane[] = [];

  if (task.sessionType === SessionType.TMUX) {
    const format = "#{window_id}\t#{window_zoomed_flag}\t#{window_width}\t#{window_height}\t#{status-position}\t#{status}";
    const [output, viewport] = await Promise.all([
      run(buildTmuxListMirrorPanesCommand(task.sessionName!)),
      run(`tmux display-message -p -t ${quoteForPosixShell(paneId)} ${quoteForPosixShell(format)}`),
    ]);
    const [tabId, zoomed, width, height, statusPosition, status] = viewport.trimEnd().split("\t");
    const statusRows = status === "on" ? 1 : status === "off" ? 0 : Number(status);
    if (!Number.isInteger(statusRows) || statusRows < 0) return unavailable;
    const topOffset = statusPosition === "top" ? statusRows : 0;
    const windowPanes = parseTmuxMirrorPaneList(output).filter((pane) => pane.tabId === tabId);
    if (!windowPanes.some((pane) => pane.id === paneId)) return unavailable;
    panes = windowPanes.filter((pane) => zoomed !== "1" || pane.id === paneId).map((pane) => ({
      id: pane.id, tabId,
      bounds: zoomed === "1"
        ? { left: 0, top: topOffset, width: Number(width), height: Number(height) }
        : { left: pane.left, top: pane.top + topOffset, width: pane.width, height: pane.height },
    }));
  } else if (task.sessionType === SessionType.ZELLIJ) {
    const [entries, tabs] = JSON.parse(await run(buildZellijReadViewportCommand(task.sessionName!)));
    if (!Array.isArray(entries) || !Array.isArray(tabs)) return unavailable;
    const tab = tabs.find((entry) => entry?.active === true);
    if (!tab) return unavailable;
    let visible = entries.filter((entry) => entry?.tab_id === tab.tab_id && entry.is_suppressed !== true);
    // 떠 있는 pane 아래의 채팅이 팝업을 가리지 않도록 최상위 레이어만 노출한다.
    if (tab.are_floating_panes_visible === true) visible = visible.filter((entry) => entry.is_floating === true);
    else visible = visible.filter((entry) => entry.is_floating !== true);
    if (tab.is_fullscreen_active === true) visible = visible.filter((entry) => entry.is_fullscreen === true);
    const frames = parseZellijMirrorPaneList(JSON.stringify(visible));
    panes = frames.map((pane) => {
      const entry = visible.find((candidate) => `terminal_${candidate.id}` === pane.id && candidate.is_plugin === false);
      const content = { left: entry.pane_content_x, top: entry.pane_content_y, width: entry.pane_content_columns, height: entry.pane_content_rows };
      return { id: pane.id, tabId: pane.tabId, bounds: hasValidPaneBounds(content) ? content : { left: pane.left, top: pane.top, width: pane.width, height: pane.height } };
    });
  } else if (task.sessionType === SessionType.HERDR) {
    const surfaceBounds = await getHerdrTaskSurfaceBounds(taskId);
    if (!surfaceBounds) return unavailable;
    const { layout } = await runHerdrCommand<{
      layout: { area: { x: number; y: number; width: number; height: number }; focused_pane_id: string; zoomed: boolean; panes: { pane_id: string; rect: { x: number; y: number; width: number; height: number } }[] };
    }>(task.sessionName!, ["pane", "layout", "--pane", paneId], task.sshHost);
    if (layout.area.width !== surfaceBounds.width || layout.area.height !== surfaceBounds.height) return unavailable;
    const owned = await listHerdrMirrorPanes(task.sessionName!, task.sshHost);
    const tabId = owned.find((pane) => pane.id === paneId)?.tabId;
    if (!tabId) return unavailable;
    panes = layout.panes.filter((pane) => owned.some((entry) => entry.id === pane.pane_id && entry.tabId === tabId) && (!layout.zoomed || pane.pane_id === paneId)).map((pane) => {
      const rect = layout.zoomed ? layout.area : pane.rect;
      return { id: pane.pane_id, tabId, bounds: { left: surfaceBounds.left + rect.x, top: surfaceBounds.top + rect.y, width: rect.width, height: rect.height } };
    });
  }

  panes = panes.filter((pane) => pane.bounds !== null && hasValidPaneBounds(pane.bounds));
  return panes.some((pane) => pane.id === paneId) ? { activePaneId: paneId, panes } : unavailable;
}
