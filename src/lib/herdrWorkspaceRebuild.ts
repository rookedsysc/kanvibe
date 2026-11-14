import { runHerdrCommand } from "@/lib/herdr";
import type { HerdrSnapshotLayout, HerdrSnapshotPane, HerdrSnapshotTab, HerdrSnapshotWorkspace } from "@/lib/herdrSnapshot";

export interface HerdrWorkspaceRebuildOptions {
  source: HerdrSnapshotWorkspace;
  sshHost: string | null;
  operationId: string;
  onCreated: (workspaceId: string) => Promise<void>;
  checkCancelled: () => void | Promise<void>;
}

export interface HerdrWorkspaceRebuildResult {
  workspaceId: string;
  paneIds: Record<string, string>;
  tabIds: string[];
}

interface LivePane { pane_id: string; workspace_id: string; tab_id: string; cwd?: string; label?: string; restore_error?: string }
interface LiveTab { tab_id: string; workspace_id: string; label: string; pane_count: number }
interface CreatedTab { tab: LiveTab; root_pane: LivePane }
interface LiveWorkspace { workspace_id: string; label: string; pane_count: number; tab_count: number; active_tab_id: string; tokens?: Record<string, string> }
interface LiveSplit { id: string; direction: string; ratio: number }
interface LiveLayout { workspace_id: string; tab_id: string; zoomed: boolean; focused_pane_id: string; panes: { pane_id: string }[]; splits: LiveSplit[] }

function firstLeaf(layout: HerdrSnapshotLayout): string {
  return layout.type === "pane" ? layout.sourcePaneId : firstLeaf(layout.first);
}

function paneFor(tab: HerdrSnapshotTab, sourcePaneId: string): HerdrSnapshotPane {
  const pane = tab.panes.find((entry) => entry.sourcePaneId === sourcePaneId);
  if (!pane) throw new Error("Herdr 재구성: 배치가 없는 pane을 참조합니다.");
  return pane;
}

function layoutSplits(layout: HerdrSnapshotLayout, path = ""): { path: string; direction: string; ratio: number }[] {
  if (layout.type === "pane") return [];
  return [{ path, direction: layout.direction, ratio: layout.ratio }, ...layoutSplits(layout.first, `${path}0`), ...layoutSplits(layout.second, `${path}1`)];
}

function layoutLeaves(layout: HerdrSnapshotLayout): string[] {
  return layout.type === "pane" ? [layout.sourcePaneId] : [...layoutLeaves(layout.first), ...layoutLeaves(layout.second)];
}

/** 공개 v0.9.5 CLI가 값을 바꾸거나 별도로 지정할 수 없는 구조는 생성 전에 차단한다. */
export function getHerdrWorkspaceRebuildBlockers(source: HerdrSnapshotWorkspace): string[] {
  const blockers: string[] = [];
  if (!source.tabs.length) return ["재구성할 탭이 없습니다."];
  if (source.cwd !== paneFor(source.tabs[0], firstLeaf(source.tabs[0].layout)).cwd) {
    blockers.push("workspace와 첫 pane의 작업 경로가 달라 정확히 재구성할 수 없습니다.");
  }
  if (source.label === "--help" || source.label === "-h") blockers.push("workspace 이름이 CLI 도움말 옵션과 충돌합니다.");
  source.tabs.forEach((tab, index) => {
    const prefix = `${index + 1}번 탭: `;
    if (tab.rootPaneId !== firstLeaf(tab.layout)) blockers.push(`${prefix}첫 pane과 다른 내부 root를 지정할 수 없습니다.`);
    if (tab.zoomed && tab.panes.length === 1) blockers.push(`${prefix}pane이 하나인 탭의 확대 상태를 복원할 수 없습니다.`);
    if (tab.label === "--help" || tab.label === "-h") blockers.push(`${prefix}이름이 CLI 도움말 옵션과 충돌합니다.`);
    if (layoutSplits(tab.layout).some((split) => !Number.isFinite(split.ratio) || split.ratio < 0.1 || split.ratio > 0.9)) {
      blockers.push(`${prefix}분할 비율은 0.1~0.9 범위만 정확히 복원할 수 있습니다.`);
    }
    for (const pane of tab.panes) {
      if (pane.cwd.startsWith("~")) blockers.push(`${prefix}홈 축약 작업 경로를 절대 경로로 바꿔야 합니다.`);
      if (pane.label !== null && (pane.label !== pane.label.trim() || ["--clear", "--help", "-h"].includes(pane.label))) {
        blockers.push(`${prefix}pane 이름을 공개 CLI로 그대로 지정할 수 없습니다.`);
      }
    }
  });
  return blockers;
}

function requireCreatedTab(created: CreatedTab, workspaceId: string, expectedTabId?: string): void {
  const validTab = typeof created.tab?.tab_id === "string" && /^w[0-9A-Za-z]+:t[0-9A-Za-z]+$/.test(created.tab.tab_id);
  const validPane = typeof created.root_pane?.pane_id === "string" && /^w[0-9A-Za-z]+:p[0-9A-Za-z]+$/.test(created.root_pane.pane_id);
  if (!validTab || !validPane || created.tab.workspace_id !== workspaceId || created.root_pane.workspace_id !== workspaceId
    || !created.tab.tab_id.startsWith(`${workspaceId}:`) || !created.root_pane.pane_id.startsWith(`${workspaceId}:`)
    || created.root_pane.tab_id !== created.tab.tab_id || (expectedTabId && created.tab.tab_id !== expectedTabId)) {
    throw new Error("Herdr 재구성: 생성 응답의 workspace/tab/pane ID가 올바르지 않습니다.");
  }
}

/** 새 shell만 만들며, 실패·취소 후에도 원본과 생성된 workspace를 보존한다. */
export async function rebuildHerdrWorkspace(options: HerdrWorkspaceRebuildOptions): Promise<HerdrWorkspaceRebuildResult> {
  const { source, sshHost, operationId, onCreated, checkCancelled } = options;
  const blockers = getHerdrWorkspaceRebuildBlockers(source);
  if (blockers.length) throw new Error(`Herdr 재구성: ${blockers.join(" ")}`);
  if (!operationId || /[\u0000-\u001f\u007f]/.test(operationId)) throw new Error("Herdr 재구성: 작업 ID가 올바르지 않습니다.");
  const command = async <T>(args: string[]): Promise<T> => {
    await checkCancelled();
    return runHerdrCommand<T>("kanvibe", args, sshHost);
  };
  const created = await command<CreatedTab & { workspace: LiveWorkspace }>([
    "workspace", "create", "--cwd", source.cwd, "--label", source.label, "--no-focus",
  ]);
  const workspaceId = created.workspace?.workspace_id;
  if (typeof workspaceId !== "string" || !/^w[0-9A-Za-z]+$/.test(workspaceId)) throw new Error("Herdr 재구성: 생성 응답의 workspace ID가 올바르지 않습니다.");
  // 취소 확인보다 먼저 저장해서 생성 직후 취소/후속 오류에도 새 workspace를 찾을 수 있게 한다.
  await onCreated(workspaceId);
  requireCreatedTab(created, workspaceId);
  await command(["workspace", "report-metadata", workspaceId, "--source", "kanvibe-session-transition", "--token", `operation=${operationId}`]);

  const paneIds: Record<string, string> = {};
  const tabIds: string[] = [];
  const usedPaneIds = new Set<string>();
  const registerPane = (tabIndex: number, sourcePaneId: string, pane: LivePane, tabId: string): string => {
    if (typeof pane?.pane_id !== "string" || !/^w[0-9A-Za-z]+:p[0-9A-Za-z]+$/.test(pane.pane_id)
      || !pane.pane_id.startsWith(`${workspaceId}:`) || pane.workspace_id !== workspaceId || pane.tab_id !== tabId || usedPaneIds.has(pane.pane_id)) {
      throw new Error("Herdr 재구성: 생성 응답의 pane ID가 올바르지 않습니다.");
    }
    usedPaneIds.add(pane.pane_id);
    paneIds[`${tabIndex}:${sourcePaneId}`] = pane.pane_id;
    return pane.pane_id;
  };
  for (const [tabIndex, tab] of source.tabs.entries()) {
    const tabCreated = tabIndex === 0 ? created : await command<CreatedTab>([
      "tab", "create", "--workspace", workspaceId, "--cwd", paneFor(tab, firstLeaf(tab.layout)).cwd,
      ...(tab.label !== null ? ["--label", tab.label] : []), "--no-focus",
    ]);
    requireCreatedTab(tabCreated, workspaceId);
    const tabId = tabCreated.tab.tab_id;
    if (tabIds.includes(tabId)) throw new Error("Herdr 재구성: 생성 응답의 tab ID가 중복되었습니다.");
    tabIds.push(tabId);
    registerPane(tabIndex, firstLeaf(tab.layout), tabCreated.root_pane, tabId);
    if (tabIndex === 0 && tab.label !== null) await command(["tab", "rename", tabId, tab.label]);
    const splitTree = async (layout: HerdrSnapshotLayout, targetPaneId: string): Promise<void> => {
      if (layout.type === "pane") return;
      const secondSourceId = firstLeaf(layout.second);
      const split = await command<{ pane: LivePane }>([
        "pane", "split", targetPaneId, "--direction", layout.direction, "--ratio", String(layout.ratio),
        "--cwd", paneFor(tab, secondSourceId).cwd, "--no-focus",
      ]);
      const secondId = registerPane(tabIndex, secondSourceId, split.pane, tabId);
      await splitTree(layout.first, targetPaneId);
      await splitTree(layout.second, secondId);
    };
    await splitTree(tab.layout, tabCreated.root_pane.pane_id);
    for (const pane of tab.panes) {
      if (pane.label !== null) await command(["pane", "rename", paneIds[`${tabIndex}:${pane.sourcePaneId}`], pane.label]);
    }
    // v0.9.5 apply_pane_zoom는 --off일 때도 지정 pane을 먼저 focus한다.
    await command(["pane", "zoom", paneIds[`${tabIndex}:${tab.focusedPaneId}`], tab.zoomed ? "--on" : "--off"]);
  }
  await command(["tab", "focus", tabIds[source.activeTabIndex]]);

  const { workspace } = await command<{ workspace: LiveWorkspace }>(["workspace", "get", workspaceId]);
  if (workspace.workspace_id !== workspaceId || workspace.label !== source.label || workspace.tab_count !== source.tabs.length
    || workspace.pane_count !== usedPaneIds.size || workspace.active_tab_id !== tabIds[source.activeTabIndex]
    || workspace.tokens?.operation !== operationId) throw new Error("Herdr 재구성 검증: workspace 수, 이름, 활성 탭 또는 작업 표시가 다릅니다.");
  const { tabs } = await command<{ tabs: LiveTab[] }>(["tab", "list", "--workspace", workspaceId]);
  if (tabs.length !== tabIds.length || tabs.some((tab, index) => tab.tab_id !== tabIds[index] || tab.workspace_id !== workspaceId
    || tab.pane_count !== source.tabs[index].panes.length || (source.tabs[index].label !== null && tab.label !== source.tabs[index].label))) {
    throw new Error("Herdr 재구성 검증: 탭 수, 순서 또는 이름이 다릅니다.");
  }
  const { panes } = await command<{ panes: LivePane[] }>(["pane", "list", "--workspace", workspaceId]);
  if (panes.length !== usedPaneIds.size || new Set(panes.map((pane) => pane.pane_id)).size !== usedPaneIds.size) throw new Error("Herdr 재구성 검증: pane 수가 다릅니다.");
  for (const [tabIndex, tab] of source.tabs.entries()) {
    for (const sourcePane of tab.panes) {
      const live = panes.find((pane) => pane.pane_id === paneIds[`${tabIndex}:${sourcePane.sourcePaneId}`]);
      if (!live || live.workspace_id !== workspaceId || live.tab_id !== tabIds[tabIndex] || live.restore_error || live.cwd !== sourcePane.cwd) {
        throw new Error("Herdr 재구성 검증: pane 작업 경로 또는 소속이 다릅니다.");
      }
      if ((live.label ?? null) !== sourcePane.label) throw new Error("Herdr 재구성 검증: pane 이름이 다릅니다.");
    }
    const { layout } = await command<{ layout: LiveLayout }>(["pane", "layout", "--pane", paneIds[`${tabIndex}:${firstLeaf(tab.layout)}`]]);
    const leaves = layoutLeaves(tab.layout).map((id) => paneIds[`${tabIndex}:${id}`]);
    const expectedSplits = layoutSplits(tab.layout);
    if (layout.workspace_id !== workspaceId || layout.tab_id !== tabIds[tabIndex] || layout.focused_pane_id !== paneIds[`${tabIndex}:${tab.focusedPaneId}`]
      || layout.zoomed !== tab.zoomed || layout.panes.length !== leaves.length || layout.panes.some((pane, index) => pane.pane_id !== leaves[index])) {
      throw new Error("Herdr 재구성 검증: pane 배치, 선택 또는 확대 상태가 다릅니다.");
    }
    const splitPaths = new Set<string>();
    if (layout.splits.length !== expectedSplits.length || layout.splits.some((split) => {
      const match = /^split_[0-9]+_(root|[01]+)$/.exec(split.id);
      const path = match?.[1] === "root" ? "" : match?.[1];
      const expected = expectedSplits.find((entry) => entry.path === path);
      if (!expected || splitPaths.has(expected.path)) return true;
      splitPaths.add(expected.path);
      return expected.direction !== split.direction || !Number.isFinite(split.ratio) || Math.abs(expected.ratio - split.ratio) > 0.000001;
    })) throw new Error("Herdr 재구성 검증: 분할 구조, 방향 또는 비율이 다릅니다.");
  }
  await checkCancelled();
  return { workspaceId, paneIds, tabIds };
}
