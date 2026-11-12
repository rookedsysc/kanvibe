import { Terminal as HeadlessTerminal } from "@xterm/headless";
import { stripVTControlCharacters } from "node:util";
import { SessionType, type KanbanTask } from "@/entities/KanbanTask";
import { execGit } from "@/lib/gitOperations";
import { listLocalTerminalTabs, readLocalTerminalTabSnapshot, readTaskTerminalAttachment, getHerdrTaskActivePaneId } from "@/lib/terminal";
import { listHerdrMirrorPanes, runHerdrCommand } from "@/lib/herdr";
import { buildTmuxCapturePaneCommand, buildZellijDumpPaneCommand, buildZellijListMirrorPanesCommand } from "@/lib/paneMirror";
import { quoteForPosixShell } from "@/lib/worktree";
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
    const panes: unknown = JSON.parse(await run(buildZellijListMirrorPanesCommand(task.sessionName)));
    if (!Array.isArray(panes)) return null;
    const pane = panes.find((entry) => entry?.is_plugin === false && (requestedPaneId ? `terminal_${entry.id}` === requestedPaneId : entry.is_focused === true));
    if (!pane) return null;
    const paneId = `terminal_${pane.id}`;
    // PID를 제공하지 않는 Zellij 버전은 정확한 화면만 반환하고 입력은 허용하지 않는다.
    const pid = pane.terminal_pid ?? pane.pid;
    return { task, paneId, rootPid: Number.isSafeInteger(pid) && pid > 0 ? pid : null, inputLine: null, cursorColumn: null, screen: capture ? stripVTControlCharacters(await run(buildZellijDumpPaneCommand(task.sessionName, paneId))) : "" };
  }
  return null;
}
