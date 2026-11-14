import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { NextIntlClientProvider } from "next-intl";
import { beforeEach, describe, expect, it, vi } from "vitest";
import HerdrSessionManager from "../HerdrSessionManager";
import {
  HerdrOperationPhase,
  HerdrSessionMode,
  type HerdrSessionOperation,
  type HerdrSessionPlan,
  type HerdrSessionScan,
} from "@/desktop/shared/herdrSessions";
import messages from "../../../../../messages/en.json";

const mocks = vi.hoisted(() => ({
  scanSessions: vi.fn(),
  createPlan: vi.fn(),
  executePlan: vi.fn(),
  getOperation: vi.fn(),
  listOperations: vi.fn(),
  controlOperation: vi.fn(),
  navigateToTaskDetail: vi.fn(),
}));
vi.mock("@/desktop/renderer/actions/herdrSessions", () => mocks);
vi.mock("@/desktop/renderer/utils/taskNavigation", () => ({
  navigateToTaskDetail: mocks.navigateToTaskDetail,
}));

const SCAN: HerdrSessionScan = {
  scanId: "scan-1", sshHost: null, observedAt: "2026-11-13T00:00:00Z", fingerprint: "fingerprint",
  sessions: [{
    name: "dotfiles-main", running: false, sessionDir: "/home/test/herdr/dotfiles-main",
    snapshotState: "valid", taskIds: ["task-1"], error: null,
    workspaces: [{ workspaceId: "w1", label: "Dotfiles", cwd: "/repo/dotfiles", tabCount: 2 }],
  }],
  tasks: [{ id: "task-1", title: "Dotfiles task", sessionName: "dotfiles-main", worktreePath: "/repo/dotfiles" }],
};
const PLAN: HerdrSessionPlan = {
  planId: "plan-1", fingerprint: "fingerprint", taskId: "task-1", mode: HerdrSessionMode.REBUILD_STOPPED,
  source: { sshHost: null, sessionName: "dotfiles-main", workspaceId: "w1" },
  targetSession: "kanvibe", eligibility: "ready",
  previousBinding: { sshHost: null, sessionType: "herdr", sessionName: "dotfiles-main", worktreePath: "/repo/dotfiles" },
  preserved: ["Working files"], recreated: ["Pane layout"], unsupported: ["Original PID"], blockers: [],
};
const OPERATION: HerdrSessionOperation = {
  operationId: "33333333-3333-4333-8333-333333333333", plan: PLAN,
  phase: HerdrOperationPhase.COMPLETED, targetWorkspaceId: "w9", backupLocation: "/backups/source.json",
  error: null, createdAt: "2026-11-13T00:00:00Z", updatedAt: "2026-11-13T00:01:00Z",
};
const success = <T,>(value: T) => ({ ok: true as const, value });
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
function mount() {
  return render(
    <NextIntlClientProvider locale="en" messages={messages}>
      <HerdrSessionManager sshHosts={["dev-vm"]} />
    </NextIntlClientProvider>,
  );
}
async function selectSource() {
  fireEvent.click(screen.getByRole("button", { name: "Scan sessions" }));
  await screen.findByRole("option", { name: /^dotfiles-main/ });
  fireEvent.change(screen.getByLabelText("Session"), { target: { value: "dotfiles-main" } });
  fireEvent.change(screen.getByLabelText("Space"), { target: { value: "w1" } });
  fireEvent.change(screen.getByLabelText("Task"), { target: { value: "task-1" } });
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.scanSessions.mockResolvedValue(success(SCAN));
  mocks.createPlan.mockResolvedValue(success(PLAN));
  mocks.executePlan.mockResolvedValue(success(OPERATION));
  mocks.listOperations.mockResolvedValue(success([]));
  mocks.getOperation.mockResolvedValue(success(OPERATION));
  mocks.controlOperation.mockResolvedValue(success({ ...OPERATION, phase: HerdrOperationPhase.REVERTED }));
  mocks.navigateToTaskDetail.mockResolvedValue(undefined);
});

describe("HerdrSessionManager", () => {
  it("uses a read-only scan before the user explicitly chooses a session, Space and task", async () => {
    mount();
    fireEvent.click(screen.getByRole("button", { name: "Scan sessions" }));
    await screen.findByRole("option", { name: /^dotfiles-main/ });
    expect(mocks.scanSessions).toHaveBeenCalledWith({ sshHost: null });
    expect(mocks.createPlan).not.toHaveBeenCalled();
    expect(mocks.executePlan).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Preview reconstruction" }).hasAttribute("disabled")).toBe(true);
  });

  it("previews preservation limits and reuses the operation ID after an uncertain transport failure", async () => {
    mocks.executePlan.mockRejectedValueOnce(new Error("Connection lost"));
    mount();
    await selectSource();
    fireEvent.click(screen.getByRole("button", { name: "Preview reconstruction" }));
    await screen.findByText("Working files");
    expect(screen.getByText("Original PID")).toBeTruthy();
    expect(mocks.createPlan).toHaveBeenCalledWith({
      scanId: "scan-1", taskId: "task-1", sourceSession: "dotfiles-main",
      sourceWorkspaceId: "w1", mode: HerdrSessionMode.REBUILD_STOPPED,
    });
    fireEvent.click(screen.getByRole("button", { name: "Execute this plan" }));
    await screen.findByText("Connection lost");
    fireEvent.click(screen.getByRole("button", { name: "Execute this plan" }));
    await screen.findByText("w9");
    expect(mocks.executePlan.mock.calls[0][0].operationId).toBe(mocks.executePlan.mock.calls[1][0].operationId);
    expect(mocks.executePlan.mock.calls[0][0].operationId).toMatch(/^[0-9a-f-]{36}$/);
    fireEvent.click(screen.getByRole("button", { name: "Open task" }));
    await waitFor(() => expect(mocks.navigateToTaskDetail).toHaveBeenCalledWith("task-1"));
  });

  it("previews an existing connection only after an explicit choice", async () => {
    mount();
    await selectSource();
    fireEvent.click(screen.getByRole("button", { name: "Preview existing connection" }));
    await waitFor(() => expect(mocks.createPlan).toHaveBeenCalledWith({
      scanId: "scan-1", taskId: "task-1", sourceSession: "dotfiles-main", sourceWorkspaceId: "w1",
      mode: HerdrSessionMode.ATTACH_EXISTING,
    }));
    expect(mocks.executePlan).not.toHaveBeenCalled();
  });

  it("can explicitly import a live Space without an existing task", async () => {
    mocks.scanSessions.mockResolvedValue(success({ ...SCAN, tasks: [],
      sessions: SCAN.sessions.map((session) => ({ ...session, running: true, taskIds: [] })),
    }));
    mocks.createPlan.mockResolvedValue(success({ ...PLAN, taskId: "new-task",
      mode: HerdrSessionMode.ATTACH_EXISTING, importTask: { title: "Imported Dotfiles" },
    }));
    mount();
    fireEvent.click(screen.getByRole("button", { name: "Scan sessions" }));
    await screen.findByRole("option", { name: /^dotfiles-main/ });
    fireEvent.change(screen.getByLabelText("Session"), { target: { value: "dotfiles-main" } });
    fireEvent.change(screen.getByLabelText("Space"), { target: { value: "w1" } });
    fireEvent.change(screen.getByLabelText("Task"), { target: { value: "__import__" } });
    fireEvent.click(screen.getByRole("button", { name: "Preview existing connection" }));
    await screen.findByText("New task: Imported Dotfiles");
    expect(mocks.createPlan).toHaveBeenCalledWith({ scanId: "scan-1", taskId: "__import__",
      sourceSession: "dotfiles-main", sourceWorkspaceId: "w1", mode: HerdrSessionMode.ATTACH_EXISTING });
    expect(mocks.executePlan).not.toHaveBeenCalled();
  });

  it("shows a blocked plan without allowing a write", async () => {
    mocks.createPlan.mockResolvedValue(success({ ...PLAN, eligibility: "blocked", blockers: ["SOURCE_RUNNING: 실행 중인 프로세스는 다른 서버로 옮길 수 없습니다."] }));
    mount();
    await selectSource();
    fireEvent.click(screen.getByRole("button", { name: "Preview reconstruction" }));
    await screen.findByText("The source session is running. Keep its existing connection.");
    expect(screen.getByRole("button", { name: "Execute this plan" }).hasAttribute("disabled")).toBe(true);
    expect(mocks.executePlan).not.toHaveBeenCalled();
  });

  it("ignores an old scan when the selected host changes", async () => {
    const pending = deferred<ReturnType<typeof success<HerdrSessionScan>>>();
    mocks.scanSessions.mockReturnValueOnce(pending.promise);
    mount();
    fireEvent.click(screen.getByRole("button", { name: "Scan sessions" }));
    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "dev-vm" } });
    await act(async () => pending.resolve(success(SCAN)));
    expect(screen.queryByRole("option", { name: /^dotfiles-main/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Scan sessions" }));
    await waitFor(() => expect(mocks.scanSessions).toHaveBeenLastCalledWith({ sshHost: "dev-vm" }));
  });

  it("discards a plan response after the user changes its source", async () => {
    const pending = deferred<ReturnType<typeof success<HerdrSessionPlan>>>();
    mocks.createPlan.mockReturnValueOnce(pending.promise);
    mount();
    await selectSource();
    fireEvent.click(screen.getByRole("button", { name: "Preview reconstruction" }));
    fireEvent.change(screen.getByLabelText("Task"), { target: { value: "" } });
    await act(async () => pending.resolve(success(PLAN)));
    expect(screen.queryByText("Working files")).toBeNull();
    expect(screen.queryByRole("button", { name: "Execute this plan" })).toBeNull();
  });

  it("loads persistent operations on mount and refreshes them when the window returns", async () => {
    mocks.listOperations.mockResolvedValue(success([OPERATION]));
    mount();
    await screen.findByRole("button", { name: /dotfiles-main.*Completed/ });
    fireEvent.focus(window);
    await waitFor(() => expect(mocks.listOperations).toHaveBeenCalledTimes(2));
    fireEvent.click(screen.getByRole("button", { name: /dotfiles-main.*Completed/ }));
    expect(screen.getByText("/backups/source.json")).toBeTruthy();
  });

  it("keeps a failed binding recovery visible instead of announcing completion", async () => {
    mocks.listOperations.mockResolvedValue(success([OPERATION]));
    mocks.controlOperation.mockResolvedValue({
      ok: false, error: { code: "REVERT_BLOCKED", message: "A new agent is running", retryable: false, operationId: OPERATION.operationId },
    });
    mount();
    fireEvent.click(await screen.findByRole("button", { name: /dotfiles-main.*Completed/ }));
    fireEvent.click(screen.getByRole("button", { name: "Restore previous connection" }));
    await screen.findByText("A new agent is running");
    expect(mocks.controlOperation).toHaveBeenCalledWith({ operationId: OPERATION.operationId, action: "revertBinding" });
    expect(screen.queryByText("Connection restored")).toBeNull();
  });

  it("allows cancellation while execution is still in flight", async () => {
    const pending = deferred<ReturnType<typeof success<HerdrSessionOperation>>>();
    mocks.executePlan.mockReturnValueOnce(pending.promise);
    const cancelled = { ...OPERATION, phase: HerdrOperationPhase.CANCELLED };
    mocks.controlOperation.mockResolvedValue(success(cancelled));
    mount();
    await selectSource();
    fireEvent.click(screen.getByRole("button", { name: "Preview reconstruction" }));
    await screen.findByText("Working files");
    fireEvent.click(screen.getByRole("button", { name: "Execute this plan" }));
    fireEvent.click(await screen.findByRole("button", { name: "Request cancellation" }));
    await waitFor(() => expect(mocks.controlOperation).toHaveBeenCalledWith({
      operationId: mocks.executePlan.mock.calls[0][0].operationId, action: "cancel",
    }));
    await act(async () => pending.resolve(success(cancelled)));
    expect(await screen.findByText("Cancelled")).toBeTruthy();
  });

  it("presents unknown operation phases as requiring inspection", async () => {
    mocks.listOperations.mockResolvedValue(success([{ ...OPERATION, phase: "newer-phase" }]));
    mount();
    expect(await screen.findByRole("button", { name: /dotfiles-main.*Needs inspection/ })).toBeTruthy();
  });
});
