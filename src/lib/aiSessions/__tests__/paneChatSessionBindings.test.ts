import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ execGit: vi.fn(), claude: vi.fn(), codex: vi.fn() }));
vi.mock("@/lib/gitOperations", () => ({ execGit: mocks.execGit }));
vi.mock("@/lib/aiSessions/readClaudeSessions", () => ({ readClaudeSessions: mocks.claude }));
vi.mock("@/lib/aiSessions/readCodexSessions", () => ({ readCodexSessions: mocks.codex }));

const started = "Thu Nov 12 19:46:51 2026";
const context = { taskId: "task-a", worktreePath: "/repo/a", repoPath: "/repo", sshHost: null };
const query = { taskId: "task-a", provider: "claude" as const, providerPid: 100, processStartTime: started };
const processTable = [
  `100 1 ${started} /usr/local/bin/claude claude`,
  `200 100 ${started} /bin/sh sh -c hook`,
  `300 200 ${started} /bin/bash bash hook.sh`,
  `101 1 ${started} /usr/local/bin/claude claude`,
  `301 101 ${started} /bin/bash bash hook.sh`,
].join("\n");
function metadata(sessionId: string, extras = {}, hookPid = 300) {
  return { provider: "claude", hookPid, payloadBase64: Buffer.from(JSON.stringify({ session_id: sessionId, cwd: "/repo/a", ...extras })).toString("base64") };
}

describe("pane chat hook session bindings", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.execGit.mockResolvedValue(processTable);
    mocks.claude.mockResolvedValue({ sessions: [{ id: "session-a", sourceRef: "/home/.claude/projects/a/session-a.jsonl" }] });
    mocks.codex.mockResolvedValue({ sessions: [] });
  });

  it("binds the exact provider ancestor and refuses another pane, host, task or process incarnation", async () => {
    const registry = await import("../paneChatSessionBindings");
    await registry.recordPaneChatSessionBinding(context, metadata("session-a"));
    expect(await registry.findPaneChatSessionBinding(query)).toEqual({ sessionId: "session-a", sourceRef: "/home/.claude/projects/a/session-a.jsonl" });
    expect(await registry.findPaneChatSessionBinding({ ...query, providerPid: 101 })).toBeNull();
    expect(await registry.findPaneChatSessionBinding({ ...query, sshHost: "remote" })).toBeNull();
    expect(await registry.findPaneChatSessionBinding({ ...query, taskId: "task-b" })).toBeNull();
    expect(await registry.findPaneChatSessionBinding({ ...query, processStartTime: "another start" })).toBeNull();
  });

  it("does not bind the newest unrelated transcript or read a hook-supplied file path", async () => {
    const registry = await import("../paneChatSessionBindings");
    await registry.recordPaneChatSessionBinding(context, metadata("session-missing", { transcript_path: "/private/secret.jsonl" }));
    expect(await registry.findPaneChatSessionBinding(query)).toBeNull();
    expect(mocks.claude).toHaveBeenCalledWith(expect.objectContaining({ worktreePath: "/repo/a", query: "session-missing" }));
    expect(JSON.stringify(mocks.claude.mock.calls)).not.toContain("/private/secret.jsonl");
  });

  it("waits for the exact first transcript flush and then resolves it", async () => {
    const registry = await import("../paneChatSessionBindings");
    mocks.claude.mockResolvedValueOnce({ sessions: [] });
    await registry.recordPaneChatSessionBinding(context, metadata("session-a"));
    expect(await registry.findPaneChatSessionBinding(query)).toBeNull();
    expect(await registry.findPaneChatSessionBinding(query)).toMatchObject({ sessionId: "session-a" });
  });

  it("keeps two Claude processes in the same directory associated with their own sessions", async () => {
    const registry = await import("../paneChatSessionBindings");
    mocks.claude.mockResolvedValue({ sessions: [{ id: "session-a", sourceRef: "/safe/a.jsonl" }, { id: "session-b", sourceRef: "/safe/b.jsonl" }] });
    await registry.recordPaneChatSessionBinding(context, metadata("session-a"));
    await registry.recordPaneChatSessionBinding(context, metadata("session-b", {}, 301));
    expect(await registry.findPaneChatSessionBinding(query)).toMatchObject({ sessionId: "session-a" });
    expect(await registry.findPaneChatSessionBinding({ ...query, providerPid: 101 })).toMatchObject({ sessionId: "session-b" });
  });

  it("ignores Codex child hooks and malformed metadata without affecting status handling", async () => {
    const registry = await import("../paneChatSessionBindings");
    await registry.recordPaneChatSessionBinding(context, { ...metadata("child", { agent_id: "worker" }), provider: "codex" });
    await registry.recordPaneChatSessionBinding(context, { provider: "claude", hookPid: 300, payloadBase64: "bad-json" });
    await registry.recordPaneChatSessionBinding(context, undefined);
    expect(mocks.execGit).not.toHaveBeenCalled();
    expect(await registry.findPaneChatSessionBinding(query)).toBeNull();
  });

  it("does not accept a hook claiming another provider or a cwd outside the task", async () => {
    const registry = await import("../paneChatSessionBindings");
    await registry.recordPaneChatSessionBinding(context, { ...metadata("session-a"), provider: "codex" });
    await registry.recordPaneChatSessionBinding(context, metadata("session-a", { cwd: "/another/project" }));
    expect(await registry.findPaneChatSessionBinding(query)).toBeNull();
  });
});
