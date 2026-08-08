import { invokeDesktop } from "@/desktop/renderer/ipc";
import type { TerminalTabListResult, TerminalTabMutationResult } from "@/desktop/shared/terminalTabs";

export function listTerminalTabs(taskId: string): Promise<TerminalTabListResult> {
  return invokeDesktop("terminalTabs", "listTerminalTabs", taskId);
}

export function createTerminalTab(taskId: string): Promise<TerminalTabMutationResult> {
  return invokeDesktop("terminalTabs", "createTerminalTab", taskId);
}

export function selectTerminalTab(taskId: string, tabId: string): Promise<TerminalTabMutationResult> {
  return invokeDesktop("terminalTabs", "selectTerminalTab", taskId, tabId);
}

export function closeTerminalTab(taskId: string, tabId: string): Promise<TerminalTabMutationResult> {
  return invokeDesktop("terminalTabs", "closeTerminalTab", taskId, tabId);
}

export function renameTerminalTab(
  taskId: string,
  tabId: string,
  name: string,
): Promise<TerminalTabMutationResult> {
  return invokeDesktop("terminalTabs", "renameTerminalTab", taskId, tabId, name);
}

export function moveTerminalTab(
  taskId: string,
  tabId: string,
  targetIndex: number,
): Promise<TerminalTabMutationResult> {
  return invokeDesktop("terminalTabs", "moveTerminalTab", taskId, tabId, targetIndex);
}
