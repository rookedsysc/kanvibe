/** @vitest-environment node */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { rebuildHerdrWorkspace, getHerdrWorkspaceRebuildBlockers } from "@/lib/herdrWorkspaceRebuild";
import { buildHerdrCommand } from "@/lib/herdr";
import type { HerdrSnapshotWorkspace } from "@/lib/herdrSnapshot";

const { execGit } = vi.hoisted(() => ({ execGit: vi.fn() }));
vi.mock("@/lib/gitOperations", () => ({ execGit, isSSHConnectionError: () => false }));

// Herdr 0.9.5 소유 격리 세션의 실제 CLI 응답에서 필요한 필드를 보존했다.
// source: herdrdev/herdr v0.9.5 src/api/schema/{panes,tabs,workspaces}.rs
const pane = (id: string, tabId: string, cwd: string, label?: string) => ({
  pane_id: id, terminal_id: `term_${id.replaceAll(":", "_")}`, workspace_id: "w1", tab_id: tabId,
  focused: false, cwd, foreground_cwd: cwd, agent_status: "unknown", revision: 0,
  scroll: { max_offset_from_bottom: 0, offset_from_bottom: 0, viewport_rows: 40 },
  ...(label ? { label } : {}),
});
const tab = (id: string, label: string, count: number, focused: boolean) => ({
  tab_id: id, workspace_id: "w1", number: id === "w1:t1" ? 1 : 2, label, focused,
  pane_count: count, agent_status: "unknown",
});
const firstRoot = pane("w1:p1", "w1:t1", "/tmp");
const secondPane = pane("w1:p2", "w1:t1", "/", "source-right");
const thirdPane = pane("w1:p3", "w1:t1", "/tmp");
const secondRoot = pane("w1:p4", "w1:t2", "/tmp");
const firstTab = tab("w1:t1", "source-first-tab", 3, true);
const secondTab = tab("w1:t2", "source-second-tab", 1, false);
const workspace = {
  workspace_id: "w1", number: 1, label: "rebuild-qa", focused: true, pane_count: 4,
  tab_count: 2, active_tab_id: "w1:t1", agent_status: "unknown", tokens: { operation: "qa-operation" },
};
const firstLayout = {
  workspace_id: "w1", tab_id: "w1:t1", zoomed: true, focused_pane_id: "w1:p2",
  area: { height: 40, width: 120, x: 0, y: 0 },
  panes: [
    { focused: false, pane_id: "w1:p1", rect: { height: 40, width: 36, x: 0, y: 0 } },
    { focused: true, pane_id: "w1:p2", rect: { height: 28, width: 84, x: 36, y: 0 } },
    { focused: false, pane_id: "w1:p3", rect: { height: 12, width: 84, x: 36, y: 28 } },
  ],
  splits: [
    { direction: "right", id: "split_0_root", ratio: 0.3, rect: { height: 40, width: 120, x: 0, y: 0 } },
    { direction: "down", id: "split_1_1", ratio: 0.7, rect: { height: 40, width: 84, x: 36, y: 0 } },
  ],
};
const secondLayout = {
  workspace_id: "w1", tab_id: "w1:t2", zoomed: false, focused_pane_id: "w1:p4",
  area: { height: 40, width: 120, x: 0, y: 0 },
  panes: [{ focused: true, pane_id: "w1:p4", rect: { height: 40, width: 120, x: 0, y: 0 } }], splits: [],
};

function source(): HerdrSnapshotWorkspace {
  return {
    sourceWorkspaceId: "wold", label: "rebuild-qa", cwd: "/tmp", activeTabIndex: 0,
    tabs: [{
      label: "source-first-tab", rootPaneId: "1", focusedPaneId: "2", zoomed: true,
      layout: { type: "split", direction: "right", ratio: 0.3, first: { type: "pane", sourcePaneId: "1" }, second: {
        type: "split", direction: "down", ratio: 0.7,
        first: { type: "pane", sourcePaneId: "2" }, second: { type: "pane", sourcePaneId: "3" },
      } },
      panes: [
        { sourcePaneId: "1", cwd: "/tmp", label: null, agentSession: null },
        { sourcePaneId: "2", cwd: "/", label: "source-right", agentSession: { source: "codex", agent: "codex", kind: "id", value: "saved-session" } },
        { sourcePaneId: "3", cwd: "/tmp", label: null, agentSession: null },
      ],
    }, {
      label: "source-second-tab", rootPaneId: "1", focusedPaneId: "1", zoomed: false,
      layout: { type: "pane", sourcePaneId: "1" },
      panes: [{ sourcePaneId: "1", cwd: "/tmp", label: null, agentSession: null }],
    }],
  };
}

function response(result: unknown): string { return JSON.stringify({ id: "cli:fixture", result }); }
function successfulResponses(panes = [firstRoot, secondPane, thirdPane, secondRoot], layout = firstLayout) {
  execGit
    .mockResolvedValueOnce(response({ type: "workspace_created", workspace: { ...workspace, tab_count: 1, pane_count: 1 }, tab: { ...firstTab, label: "1", pane_count: 1 }, root_pane: firstRoot }))
    .mockResolvedValueOnce("")
    .mockResolvedValueOnce(response({ type: "tab_info", tab: firstTab }))
    .mockResolvedValueOnce(response({ type: "pane_info", pane: secondPane }))
    .mockResolvedValueOnce(response({ type: "pane_info", pane: thirdPane }))
    .mockResolvedValueOnce(response({ type: "pane_info", pane: secondPane }))
    .mockResolvedValueOnce(response({ type: "pane_zoom", zoom: { layout: firstLayout } }))
    .mockResolvedValueOnce(response({ type: "tab_created", tab: secondTab, root_pane: secondRoot }))
    .mockResolvedValueOnce(response({ type: "pane_zoom", zoom: { layout: secondLayout } }))
    .mockResolvedValueOnce(response({ type: "tab_info", tab: firstTab }))
    .mockResolvedValueOnce(response({ type: "workspace_info", workspace }))
    .mockResolvedValueOnce(response({ type: "tab_list", tabs: [firstTab, secondTab] }))
    .mockResolvedValueOnce(response({ type: "pane_list", panes }))
    .mockResolvedValueOnce(response({ type: "pane_layout", layout }))
    .mockResolvedValueOnce(response({ type: "pane_layout", layout: secondLayout }));
}
const options = () => ({ source: source(), sshHost: "dev-vm", operationId: "qa-operation", onCreated: vi.fn(async () => {}), checkCancelled: vi.fn() });
beforeEach(() => { execGit.mockReset(); });

describe("Herdr snapshot workspace 재구성", () => {
  it("실제 CLI 응답으로 탭, 분할, cwd, label, focus와 zoom을 복원하고 pane ID 충돌을 구분한다", async () => {
    successfulResponses();
    const input = options();
    const result = await rebuildHerdrWorkspace(input);

    expect(result).toEqual({ workspaceId: "w1", tabIds: ["w1:t1", "w1:t2"], paneIds: {
      "0:1": "w1:p1", "0:2": "w1:p2", "0:3": "w1:p3", "1:1": "w1:p4",
    } });
    expect(input.onCreated).toHaveBeenCalledWith("w1");
    expect(execGit).toHaveBeenCalledWith(buildHerdrCommand("kanvibe", ["workspace", "create", "--cwd", "/tmp", "--label", "rebuild-qa", "--no-focus"]), "dev-vm", { timeoutMs: 5_000, retrySafe: false, shouldLogFailure: true });
    expect(execGit).toHaveBeenCalledWith(buildHerdrCommand("kanvibe", ["pane", "split", "w1:p2", "--direction", "down", "--ratio", "0.7", "--cwd", "/tmp", "--no-focus"]), "dev-vm", { timeoutMs: 5_000, retrySafe: false, shouldLogFailure: true });
    expect(execGit.mock.calls.every(([command]) => !/send-text|send-keys|agent.*start|workspace.*close|session.*stop/.test(command))).toBe(true);
  });

  it("생성 ID 저장 callback이 끝나기 전에는 대상에 추가 변경을 하지 않는다", async () => {
    successfulResponses();
    const input = options();
    let release!: () => void;
    input.onCreated.mockImplementation(() => new Promise<void>((resolve) => { release = resolve; }));
    const pending = rebuildHerdrWorkspace(input);
    await vi.waitFor(() => expect(input.onCreated).toHaveBeenCalledWith("w1"));
    expect(execGit).toHaveBeenCalledTimes(1);
    release();
    await pending;
  });

  it("생성 직후 취소되어도 생성 ID를 먼저 저장하고 새 workspace를 유지한다", async () => {
    successfulResponses();
    const input = options();
    input.onCreated.mockImplementation(async () => { input.checkCancelled.mockImplementation(() => { throw new Error("cancelled"); }); });
    await expect(rebuildHerdrWorkspace(input)).rejects.toThrow("cancelled");
    expect(input.onCreated).toHaveBeenCalledWith("w1");
    expect(execGit).toHaveBeenCalledTimes(1);
  });

  it("생성 ID 저장 실패 후에는 새 workspace를 닫거나 수정하지 않는다", async () => {
    successfulResponses();
    const input = options();
    input.onCreated.mockRejectedValue(new Error("journal unavailable"));
    await expect(rebuildHerdrWorkspace(input)).rejects.toThrow("journal unavailable");
    expect(execGit).toHaveBeenCalledTimes(1);
  });

  it("다른 workspace의 pane ID를 받은 경우에는 그 pane을 수정하지 않는다", async () => {
    successfulResponses();
    execGit.mockReset().mockResolvedValueOnce(response({ workspace, tab: firstTab, root_pane: { ...firstRoot, pane_id: "w2:p1" } }));
    const input = options();
    await expect(rebuildHerdrWorkspace(input)).rejects.toThrow("생성 응답");
    expect(input.onCreated).toHaveBeenCalledWith("w1");
    expect(execGit).toHaveBeenCalledTimes(1);
  });

  it("검증 시 작업 경로가 다르면 완료로 보고하지 않고 대상을 유지한다", async () => {
    successfulResponses([{ ...firstRoot, cwd: "/wrong" }, secondPane, thirdPane, secondRoot]);
    await expect(rebuildHerdrWorkspace(options())).rejects.toThrow("작업 경로");
    expect(execGit.mock.calls.some(([command]) => command.includes("'close'"))).toBe(false);
  });

  it("동일한 pane 수라도 분할 트리 경로가 다르면 검증에 실패한다", async () => {
    successfulResponses(undefined, { ...firstLayout, splits: [firstLayout.splits[0], { ...firstLayout.splits[1], id: "split_1_0" }] });
    await expect(rebuildHerdrWorkspace(options())).rejects.toThrow("분할");
  });
});

describe("정확히 재구성할 수 없는 snapshot 사전검사", () => {
  it("현재 공개 CLI로 보장 가능한 snapshot에는 blocker가 없다", () => {
    expect(getHerdrWorkspaceRebuildBlockers(source())).toEqual([]);
  });
  it.each([0.099, 0.901])("Herdr가 clamp하는 비율 %s를 생성 전에 거부한다", async (ratio) => {
    const input = options();
    if (input.source.tabs[0].layout.type !== "split") throw new Error("fixture");
    input.source.tabs[0].layout.ratio = ratio;
    expect(getHerdrWorkspaceRebuildBlockers(input.source).join(" ")).toContain("비율");
    await expect(rebuildHerdrWorkspace(input)).rejects.toThrow("비율");
    expect(execGit).not.toHaveBeenCalled();
  });
  it("첫 leaf와 다른 내부 root를 별도로 지정할 수 없으면 거부한다", () => {
    const input = source(); input.tabs[0].rootPaneId = "2";
    expect(getHerdrWorkspaceRebuildBlockers(input).join(" ")).toContain("root");
  });
  it("workspace identity cwd와 첫 pane cwd가 다르면 거부한다", () => {
    const input = source(); input.cwd = "/different";
    expect(getHerdrWorkspaceRebuildBlockers(input).join(" ")).toContain("작업 경로");
  });
  it("Herdr가 무시하는 single-pane zoom을 거부한다", () => {
    const input = source(); input.tabs[1].zoomed = true;
    expect(getHerdrWorkspaceRebuildBlockers(input).join(" ")).toContain("확대");
  });
});
