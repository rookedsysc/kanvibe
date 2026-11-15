import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ execGit: vi.fn(), claude: vi.fn(), codex: vi.fn(), home: vi.fn(), readFile: vi.fn(), exists: vi.fn() }));
vi.mock("@/lib/gitOperations", () => ({ execGit: mocks.execGit }));
vi.mock("@/lib/aiSessions/readClaudeSessions", () => ({ readClaudeSessions: mocks.claude }));
vi.mock("@/lib/aiSessions/readCodexSessions", () => ({ readCodexSessions: mocks.codex }));

vi.mock("@/lib/hostFileAccess", () => ({ getHomeDirectory: mocks.home, readTextFile: mocks.readFile, pathExists: mocks.exists }));

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
    expect(await registry.findPaneChatSessionBinding(query)).toMatchObject({ sessionId: "session-a", sourceRef: "/home/.claude/projects/a/session-a.jsonl" });
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


it("never records a shared Codex daemon as the owning TUI", async () => {
  vi.resetModules();
  mocks.execGit.mockResolvedValue(`100 1 ${started} /usr/local/bin/codex codex app-server --managed-daemon\n300 100 ${started} /bin/bash bash hook.sh`);
  mocks.codex.mockResolvedValue({ sessions: [{ id: "session-a", sourceRef: "/safe/a.jsonl" }] });
  const registry = await import("../paneChatSessionBindings");
  await registry.recordPaneChatSessionBinding(context, { ...metadata("session-a"), provider: "codex" });
  expect(await registry.findPaneChatSessionBinding({ ...query, provider: "codex" })).toBeNull();
});


it("fences A to B to A even when no consumer queried B", async () => {
  vi.resetModules();
  mocks.execGit.mockResolvedValue(processTable);
  mocks.claude.mockResolvedValue({ sessions: [{ id: "session-a", sourceRef: "/safe/a.jsonl" }, { id: "session-b", sourceRef: "/safe/b.jsonl" }] });
  const registry = await import("../paneChatSessionBindings");
  await registry.recordPaneChatSessionBinding(context, metadata("session-a"));
  const first = await registry.findPaneChatSessionBinding(query);
  await registry.recordPaneChatSessionBinding(context, metadata("session-a"));
  expect((await registry.findPaneChatSessionBinding(query))?.revision).toBe(first?.revision);
  await registry.recordPaneChatSessionBinding(context, metadata("session-b"));
  await registry.recordPaneChatSessionBinding(context, metadata("session-a"));
  const returned = await registry.findPaneChatSessionBinding(query);
  expect(returned?.sessionId).toBe(first?.sessionId);
  expect(returned?.revision).not.toBe(first?.revision);
});


describe("passive process and reported session bindings", () => {
  const process = { pid: 100, processStartTime: started, provider: "claude" as const };
  const descriptor = { pid: 100, sessionId: "session-a", cwd: "/repo/a", kind: "interactive", procStart: "12345", pidDomain: "linux:machine:pid:[1]" };
  const linuxStat = `100 (claude (native)) S ${Array(18).fill("0").join(" ")} 12345 0`;
  beforeEach(() => {
    vi.resetModules(); vi.clearAllMocks();
    mocks.home.mockResolvedValue("/home/user");
    mocks.exists.mockResolvedValue(true);
    mocks.readFile.mockResolvedValue(JSON.stringify(descriptor));
    mocks.execGit.mockResolvedValue(`${linuxStat}\nlinux:machine:pid:[1]\n`);
    mocks.claude.mockResolvedValue({ sessions: [{ id: "session-a", sourceRef: "/safe/session-a.jsonl" }] });
    mocks.codex.mockResolvedValue({ sessions: [{ id: "codex-a", sourceRef: "/safe/codex-a.jsonl" }] });
  });
  it("binds an idle Claude's exact live PID record without hooks, lsof or CLI input", async () => {
    const registry = await import("../paneChatSessionBindings");
    expect(await registry.findPaneChatProcessSessionBinding(context, process)).toEqual({ sessionId: "session-a", sourceRef: "/safe/session-a.jsonl" });
    expect(mocks.readFile).toHaveBeenCalledWith("/home/user/.claude/sessions/100.json", null);
    expect(mocks.execGit.mock.calls.every(([command]) => !/lsof|send-|statusline/.test(command))).toBe(true);
  });
  it("allows the exact newly started empty session before its first JSONL flush", async () => {
    mocks.exists.mockResolvedValue(false);
    const registry = await import("../paneChatSessionBindings");
    expect(await registry.findPaneChatProcessSessionBinding(context, process)).toEqual({ sessionId: "session-a", sourceRef: "/home/user/.claude/projects/-repo-a/session-a.jsonl", pending: true });
    expect(mocks.claude).not.toHaveBeenCalled();
  });
  it.each([
    { pid: 101 }, { procStart: "old-start" }, { pidDomain: "linux:other:pid:[1]" },
    { cwd: "/different/repo" }, { kind: "background" }, { sessionId: "../unsafe" }, { procStart: null },
  ])("rejects foreign or stale process records %j", async (invalid) => {
    mocks.readFile.mockResolvedValue(JSON.stringify({ ...descriptor, ...invalid }));
    const registry = await import("../paneChatSessionBindings");
    expect(await registry.findPaneChatProcessSessionBinding(context, process)).toBeNull();
    expect(mocks.claude).not.toHaveBeenCalled();
  });
  it("validates the UTC process start marker on macOS", async () => {
    mocks.execGit.mockResolvedValue(`${started}\nlinux\n`);
    mocks.readFile.mockResolvedValue(JSON.stringify({ ...descriptor, procStart: started, pidDomain: "linux" }));
    const registry = await import("../paneChatSessionBindings");
    expect(await registry.findPaneChatProcessSessionBinding(context, process)).toMatchObject({ sessionId: "session-a" });
    expect(mocks.execGit).toHaveBeenCalledWith(expect.stringContaining("LC_ALL=C TZ=UTC ps -o lstart="), null, expect.anything());
  });
  it("rereads the process session after clear or resume instead of caching the old ID", async () => {
    const registry = await import("../paneChatSessionBindings");
    expect(await registry.findPaneChatProcessSessionBinding(context, process)).toMatchObject({ sessionId: "session-a" });
    mocks.readFile.mockResolvedValue(JSON.stringify({ ...descriptor, sessionId: "session-b" }));
    mocks.claude.mockResolvedValue({ sessions: [{ id: "session-b", sourceRef: "/safe/session-b.jsonl" }] });
    expect(await registry.findPaneChatProcessSessionBinding(context, process)).toMatchObject({ sessionId: "session-b" });
  });
  it("keeps an unavailable or malformed record unavailable without selecting a recent session", async () => {
    const registry = await import("../paneChatSessionBindings");
    mocks.readFile.mockRejectedValueOnce(new Error("missing"));
    expect(await registry.findPaneChatProcessSessionBinding(context, process)).toBeNull();
    mocks.readFile.mockResolvedValueOnce("not json");
    expect(await registry.findPaneChatProcessSessionBinding(context, process)).toBeNull();
    expect(await registry.findPaneChatProcessSessionBinding(context, { ...process, provider: "codex" })).toBeNull();
  });
  it("resolves only the reported provider's exact allowed transcript ID", async () => {
    const registry = await import("../paneChatSessionBindings");
    expect(await registry.findPaneChatReportedSessionBinding(context, "codex", "codex-a")).toEqual({ sessionId: "codex-a", sourceRef: "/safe/codex-a.jsonl" });
    expect(await registry.findPaneChatReportedSessionBinding(context, "claude", "missing")).toBeNull();
    expect(await registry.findPaneChatReportedSessionBinding(context, "claude", "../unsafe")).toBeNull();
    expect(mocks.codex).toHaveBeenCalledWith(expect.objectContaining({ query: "codex-a" }));
  });
});
