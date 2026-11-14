import path from "node:path";

export interface HerdrSnapshotAgentSession {
  source: string;
  agent: string;
  kind: "id" | "path";
  value: string;
}

export interface HerdrSnapshotPane {
  sourcePaneId: string;
  cwd: string;
  label: string | null;
  agentSession: HerdrSnapshotAgentSession | null;
}

export type HerdrSnapshotLayout =
  | { type: "pane"; sourcePaneId: string }
  | { type: "split"; direction: "right" | "down"; ratio: number; first: HerdrSnapshotLayout; second: HerdrSnapshotLayout };

export interface HerdrSnapshotTab {
  label: string | null;
  layout: HerdrSnapshotLayout;
  panes: HerdrSnapshotPane[];
  focusedPaneId: string;
  rootPaneId: string;
  zoomed: boolean;
}

export interface HerdrSnapshotWorkspace {
  sourceWorkspaceId: string;
  label: string;
  cwd: string;
  activeTabIndex: number;
  tabs: HerdrSnapshotTab[];
}

export interface HerdrSnapshot {
  version: 3;
  workspaces: HerdrSnapshotWorkspace[];
}

// 정본: herdrdev/herdr v0.9.5 src/persist/snapshot.rs. 현재 저장 형식만 명시적으로 지원한다.
const SNAPSHOT_VERSION = 3;
const MAX_LAYOUT_DEPTH = 64;
const MAX_PANE_ID = 0xffff_ffff;

function invalid(message: string): never {
  throw new Error(`Herdr snapshot: ${message}`);
}

function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) invalid("자료 형식이 올바르지 않습니다.");
  return value as Record<string, unknown>;
}

function text(value: unknown, label: string): string {
  if (typeof value !== "string" || !value || /[\u0000-\u001f\u007f]/.test(value)) invalid(`${label} 값이 올바르지 않습니다.`);
  return value;
}

function optionalText(value: unknown): string | null {
  if (value === undefined || value === null) return null;
  return text(value, "이름");
}

function workingDirectory(value: unknown): string {
  const directory = text(value, "작업 경로");
  if (!path.posix.isAbsolute(directory) && directory !== "~" && !directory.startsWith("~/")) invalid("작업 경로는 절대 경로나 홈 경로여야 합니다.");
  return directory;
}

function paneId(value: unknown): string {
  if (typeof value !== "number" || !Number.isInteger(value) || value < 0 || value > MAX_PANE_ID) invalid("pane ID가 올바르지 않습니다.");
  return String(value);
}

function readAgentSession(value: unknown): HerdrSnapshotAgentSession | null {
  if (value === undefined || value === null) return null;
  const session = record(value);
  if (session.kind !== "id" && session.kind !== "path") invalid("에이전트 세션 참조 형식을 지원하지 않습니다.");
  return {
    source: text(session.source, "에이전트 참조 출처"),
    agent: text(session.agent, "에이전트"),
    kind: session.kind,
    value: session.kind === "path" ? workingDirectory(session.value) : text(session.value, "에이전트 세션 ID"),
  };
}

function readPanes(value: unknown): HerdrSnapshotPane[] {
  return Object.entries(record(value)).map(([id, raw]) => {
    if (!/^(0|[1-9][0-9]*)$/.test(id) || Number(id) > MAX_PANE_ID) invalid("pane ID가 올바르지 않습니다.");
    const pane = record(raw);
    return {
      sourcePaneId: id,
      cwd: workingDirectory(pane.cwd),
      label: optionalText(pane.label),
      agentSession: readAgentSession(pane.agent_session),
    };
  });
}

function readLayout(value: unknown, paneIds: Set<string>, used: Set<string>, depth = 0): HerdrSnapshotLayout {
  if (depth > MAX_LAYOUT_DEPTH) invalid("분할 배치가 너무 깊습니다.");
  const layout = record(value);
  if (Object.keys(layout).length !== 1) invalid("분할 배치 형식이 올바르지 않습니다.");
  if (Object.hasOwn(layout, "Pane")) {
    const id = paneId(layout.Pane);
    if (!paneIds.has(id)) invalid("배치가 없는 pane을 참조합니다.");
    if (used.has(id)) invalid("배치의 pane ID가 중복되었습니다.");
    used.add(id);
    return { type: "pane", sourcePaneId: id };
  }
  if (!Object.hasOwn(layout, "Split")) invalid("분할 배치 형식을 지원하지 않습니다.");
  const split = record(layout.Split);
  if (split.direction !== "Horizontal" && split.direction !== "Vertical") invalid("분할 방향을 지원하지 않습니다.");
  if (typeof split.ratio !== "number" || !Number.isFinite(split.ratio) || split.ratio <= 0 || split.ratio >= 1) invalid("분할 비율이 올바르지 않습니다.");
  return {
    type: "split",
    direction: split.direction === "Horizontal" ? "right" : "down",
    ratio: split.ratio,
    first: readLayout(split.first, paneIds, used, depth + 1),
    second: readLayout(split.second, paneIds, used, depth + 1),
  };
}

function selectedPane(value: unknown, panes: Set<string>, fallback: string): string {
  if (value === undefined || value === null) return fallback;
  const id = paneId(value);
  if (!panes.has(id)) invalid("선택한 pane이 없습니다.");
  return id;
}

function firstPane(layout: HerdrSnapshotLayout): string {
  return layout.type === "pane" ? layout.sourcePaneId : firstPane(layout.first);
}

function readTab(value: unknown): HerdrSnapshotTab {
  const tab = record(value);
  const panes = readPanes(tab.panes);
  const ids = new Set(panes.map((pane) => pane.sourcePaneId));
  const used = new Set<string>();
  const layout = readLayout(tab.layout, ids, used);
  if (used.size !== ids.size) invalid("배치에 없는 pane 자료가 있습니다.");
  if (typeof tab.zoomed !== "boolean") invalid("확대 상태 형식이 올바르지 않습니다.");
  const fallback = firstPane(layout);
  return {
    label: optionalText(tab.custom_name), layout, panes,
    focusedPaneId: selectedPane(tab.focused, ids, fallback),
    rootPaneId: selectedPane(tab.root_pane, ids, fallback),
    zoomed: tab.zoomed,
  };
}

function readWorkspace(value: unknown, workspaceIds: Set<string>): HerdrSnapshotWorkspace {
  const workspace = record(value);
  const id = text(workspace.id, "workspace ID");
  if (!/^w[0-9A-Za-z]+$/.test(id)) invalid("workspace ID가 올바르지 않습니다.");
  if (workspaceIds.has(id)) invalid("workspace ID가 중복되었습니다.");
  workspaceIds.add(id);
  if (!Array.isArray(workspace.tabs) || workspace.tabs.length === 0) invalid("작업 공간에 탭이 없습니다.");
  const tabs = workspace.tabs.map(readTab);
  const active = workspace.active_tab === undefined ? 0 : workspace.active_tab;
  if (typeof active !== "number" || !Number.isInteger(active) || active < 0 || active >= tabs.length) invalid("활성 탭이 없습니다.");
  return {
    sourceWorkspaceId: id,
    label: optionalText(workspace.custom_name) ?? id,
    cwd: workingDirectory(workspace.identity_cwd),
    activeTabIndex: active,
    tabs,
  };
}

/** 저장된 실행 명령은 노출하지 않고 재구성에 필요한 구조와 대화 참조만 읽는다. */
export function decodeHerdrSnapshot(content: string): HerdrSnapshot {
  let parsed: unknown;
  try {
    parsed = JSON.parse(content);
  } catch {
    invalid("JSON 형식이 올바르지 않습니다.");
  }
  const snapshot = record(parsed);
  if (snapshot.version !== SNAPSHOT_VERSION) invalid("snapshot version 3 형식만 지원합니다.");
  if (!Array.isArray(snapshot.workspaces)) invalid("작업 공간 목록 형식이 올바르지 않습니다.");
  const workspaceIds = new Set<string>();
  return { version: SNAPSHOT_VERSION, workspaces: snapshot.workspaces.map((workspace) => readWorkspace(workspace, workspaceIds)) };
}
