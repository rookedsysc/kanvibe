import { invokeDesktop } from "@/desktop/renderer/ipc";
import { triggerDesktopRefresh } from "@/desktop/renderer/utils/refresh";
import type { PaneLayoutConfig, PaneLayoutType, PaneCommand } from "@/entities/PaneLayoutConfig";

export interface SavePaneLayoutInput {
  layoutType: PaneLayoutType;
  panes: PaneCommand[];
  projectId?: string | null;
  isGlobal?: boolean;
}

async function invokeAndRefresh<T>(method: string, ...args: unknown[]): Promise<T> {
  const result = await invokeDesktop<T>("paneLayout", method, ...args);
  triggerDesktopRefresh("pane-layout");
  return result;
}

export function getGlobalPaneLayout(): Promise<PaneLayoutConfig | null> {
  return invokeDesktop("paneLayout", "getGlobalPaneLayout");
}

export function getProjectPaneLayout(projectId: string): Promise<PaneLayoutConfig | null> {
  return invokeDesktop("paneLayout", "getProjectPaneLayout", projectId);
}

export function getEffectivePaneLayout(projectId?: string): Promise<PaneLayoutConfig | null> {
  return invokeDesktop("paneLayout", "getEffectivePaneLayout", projectId);
}

export function getAllPaneLayouts(): Promise<PaneLayoutConfig[]> {
  return invokeDesktop("paneLayout", "getAllPaneLayouts");
}

export function savePaneLayout(input: SavePaneLayoutInput): Promise<PaneLayoutConfig> {
  return invokeAndRefresh("savePaneLayout", input);
}

export function deletePaneLayout(id: string): Promise<boolean> {
  return invokeAndRefresh("deletePaneLayout", id);
}
