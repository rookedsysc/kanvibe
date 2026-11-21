import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  addMobileAiAccount,
  getMobileAgentCallGraph,
  getMobileAiUsageSnapshot,
  getMobileLiveAiSessions,
  getMobileRunningAgentPanes,
  listMobileAiAccounts,
  openMobileAiLogin,
  removeMobileAiAccount,
} from "../mobileAiService";

const mocks = vi.hoisted(() => ({
  list: vi.fn(), add: vi.fn(), remove: vi.fn(), usage: vi.fn(), task: vi.fn(), live: vi.fn(), graph: vi.fn(),
  panes: vi.fn(), open: vi.fn(), closeAccount: vi.fn(),
}));
vi.mock("../aiAccountService", () => ({ listAiAccounts: mocks.list, addAiAccount: mocks.add, removeAiAccount: mocks.remove }));
vi.mock("../aiUsageService", () => ({ getAiUsageSnapshot: mocks.usage }));
vi.mock("../kanbanService", () => ({ getTaskById: mocks.task }));
vi.mock("../projectService", () => ({ getRunningAgentPanes: mocks.panes, getTaskLiveAiSessions: mocks.live, getTaskAgentCallGraph: mocks.graph }));
vi.mock("../../mobileAiLoginBridge", () => ({
  openMobileAiLoginSession: mocks.open, closeMobileAiLoginsForAccount: mocks.closeAccount,
  writeMobileAiLogin: vi.fn(), resizeMobileAiLogin: vi.fn(), closeMobileAiLogin: vi.fn(),
  closeMobileAiLoginsForDevice: vi.fn(), closeAllMobileAiLogins: vi.fn(),
}));
const account = { provider: "claude", accountRoot: "/fixture/.claude-work", accountName: "work", label: "work@example.test", isLoggedIn: true, planName: "pro", isRemovable: true };
beforeEach(() => {
  vi.resetAllMocks();
  mocks.list.mockResolvedValue([account]);
  mocks.task.mockResolvedValue({ id: "task-1" });
  mocks.add.mockResolvedValue({ outcome: "ok", accountRoot: account.accountRoot });
  mocks.remove.mockResolvedValue({ outcome: "ok" });
});

describe("mobile AI service", () => {
  it("lists opaque account handles without leaking filesystem roots or extra fields", async () => {
    mocks.list.mockResolvedValue([{ ...account, accessToken: "should-not-leak" }]);
    const result = await listMobileAiAccounts();
    expect(result).toEqual([{ accountId: expect.stringMatching(/^[a-f0-9]{64}$/), provider: "claude", accountName: "work", label: account.label, isLoggedIn: true, planName: "pro", isRemovable: true }]);
    expect(JSON.stringify(result)).not.toContain(account.accountRoot);
    expect(JSON.stringify(result)).not.toContain("should-not-leak");
    expect(await listMobileAiAccounts()).toEqual(result);
  });
  it("creates only validated named provider accounts and returns their opaque handle", async () => {
    const result = await addMobileAiAccount({ provider: "claude", accountName: "work" });
    expect(mocks.add).toHaveBeenCalledWith("claude", "work");
    expect(result.accountId).toBe((await listMobileAiAccounts())[0].accountId);
  });
  it.each([
    null, [], { provider: "unknown", accountName: "work" }, { provider: "claude", accountName: "../escape" },
    { provider: "claude", accountName: "work", accountRoot: "/etc" }, { provider: "claude", accountName: 5 },
  ])("rejects untrusted account creation before writes: %j", async (input) => {
    await expect(addMobileAiAccount(input)).rejects.toMatchObject({ statusCode: 400 });
    expect(mocks.add).not.toHaveBeenCalled();
  });
  it("reports duplicate accounts as conflict", async () => {
    mocks.add.mockResolvedValue({ outcome: "already-exists" });
    await expect(addMobileAiAccount({ provider: "claude", accountName: "work" })).rejects.toMatchObject({ statusCode: 409 });
  });
  it("resolves deletion handles back to a current server account root", async () => {
    const id = (await listMobileAiAccounts())[0].accountId;
    await expect(removeMobileAiAccount(id)).resolves.toEqual({ outcome: "ok" });
    expect(mocks.closeAccount).toHaveBeenCalledWith(account.accountRoot);
    expect(mocks.remove).toHaveBeenCalledWith("claude", account.accountRoot);
  });
  it("rejects path injection and stale handles without deleting accounts", async () => {
    await expect(removeMobileAiAccount("/fixture/.claude-work")).rejects.toMatchObject({ statusCode: 400 });
    await expect(removeMobileAiAccount("a".repeat(64))).rejects.toMatchObject({ statusCode: 404 });
    expect(mocks.remove).not.toHaveBeenCalled();
  });
  it("protects discovered accounts that KanVibe does not own", async () => {
    mocks.list.mockResolvedValue([{ ...account, isRemovable: false }]);
    const id = (await listMobileAiAccounts())[0].accountId;
    await expect(removeMobileAiAccount(id)).rejects.toMatchObject({ statusCode: 403 });
    expect(mocks.remove).not.toHaveBeenCalled();
  });
  it("uses the validated server account for login and never accepts a provider/path override", async () => {
    const id = (await listMobileAiAccounts())[0].accountId;
    const callbacks = { isOpen: () => true, onEvent: vi.fn() };
    await openMobileAiLogin("device-1", "socket-1", id, 80, 24, callbacks);
    expect(mocks.open).toHaveBeenCalledWith(expect.objectContaining({ deviceId: "device-1", socketId: "socket-1", provider: "claude", accountRoot: account.accountRoot, cols: 80, rows: 24, ...callbacks }));
  });
  it("does not launch login after the requesting socket closes during account lookup", async () => {
    const id = (await listMobileAiAccounts())[0].accountId;
    await openMobileAiLogin("device-1", "socket-1", id, 80, 24, { isOpen: () => false, onEvent: vi.fn() });
    expect(mocks.open).not.toHaveBeenCalled();
  });
  it("uses existing live and graph readers only for known tasks and valid providers", async () => {
    mocks.live.mockResolvedValue({ taskId: "task-1", sessions: [] });
    mocks.graph.mockResolvedValue(null);
    expect(await getMobileLiveAiSessions("task-1")).toEqual({ taskId: "task-1", sessions: [] });
    expect(await getMobileAgentCallGraph("task-1", "claude", "session-1")).toBeNull();
    expect(mocks.graph).toHaveBeenCalledWith("task-1", "claude", "session-1");
    await expect(getMobileAgentCallGraph("task-1", "shell", "session-1")).rejects.toMatchObject({ statusCode: 400 });
    mocks.task.mockResolvedValue(null);
    await expect(getMobileLiveAiSessions("missing")).rejects.toMatchObject({ statusCode: 404 });
  });
  it.each(["../other", "/etc/account", "folder\\session", ".", ".."])("rejects graph session path injection: %s", async (sessionId) => {
    await expect(getMobileAgentCallGraph("task-1", "claude", sessionId)).rejects.toMatchObject({ statusCode: 400 });
    expect(mocks.graph).not.toHaveBeenCalled();
  });
  it("serializes account deletion with login startup so a lookup cannot revive a deleted account", async () => {
    const id = (await listMobileAiAccounts())[0].accountId;
    let finishRemoval!: () => void;
    mocks.remove.mockImplementation(() => new Promise((resolve) => {
      finishRemoval = () => { mocks.list.mockResolvedValue([]); resolve({ outcome: "ok" }); };
    }));
    const removing = removeMobileAiAccount(id);
    await vi.waitFor(() => expect(mocks.remove).toHaveBeenCalledTimes(1));
    const opening = openMobileAiLogin("device-1", "socket-1", id, 80, 24, { isOpen: () => true, onEvent: vi.fn() });
    const failedOpening = expect(opening).rejects.toMatchObject({ statusCode: 404 });
    finishRemoval();
    await removing;
    await failedOpening;
    expect(mocks.open).not.toHaveBeenCalled();
  });
  it("serializes usage with hashed identity and an explicit public field allowlist", async () => {
    mocks.usage.mockResolvedValue({ fetchedAt: "2026-11-13T00:00:00Z", secret: "hidden", accounts: [{ provider: "claude", accountId: "/fixture/.claude-work", label: "work", status: "ok", planName: "pro", reason: null, fetchedAt: "2026-11-13T00:00:00Z", token: "hidden", windows: [{ kind: "weekly", modelName: null, usedPercent: 42, resetsAt: null, token: "hidden" }] }] });
    const result = await getMobileAiUsageSnapshot();
    expect(result.accounts[0].windows).toEqual([{ kind: "weekly", modelName: null, usedPercent: 42, resetsAt: null }]);
    expect(result.accounts[0].accountId).toMatch(/^[a-f0-9]{64}$/);
    expect(JSON.stringify(result)).not.toContain("hidden");
    expect(JSON.stringify(result)).not.toContain("/fixture");
  });
});

it("returns the shared running pane snapshot", async () => {
  mocks.panes.mockResolvedValue([{ provider: "codex", worktreePath: "/work", windowId: "@1" }]);
  expect(await getMobileRunningAgentPanes()).toEqual([{ provider: "codex", worktreePath: "/work", windowId: "@1" }]);
});
