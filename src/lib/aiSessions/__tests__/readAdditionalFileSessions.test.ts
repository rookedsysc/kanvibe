import { execFile } from "child_process";
import { promisify } from "util";
import { mkdir, mkdtemp, rm, writeFile, symlink } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readTextFile } from "@/lib/hostFileAccess";
import { readAdditionalFileSessions, readAdditionalFileSessionDetail } from "@/lib/aiSessions/readAdditionalFileSessions";

const host = vi.hoisted(() => ({ home: "" }));
vi.mock("@/lib/hostFileAccess", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/hostFileAccess")>();
  return { ...actual, getHomeDirectory: async () => host.home, readTextFile: vi.fn(actual.readTextFile) };
});
const cwd = "/projects/history-fixture";
const at = "2026-11-01T10:00:00Z";
const later = "2026-11-01T10:01:00Z";
const user = "Please explain the change";
const answer = "The change is complete";
const jsonl = (values: unknown[]) => values.map((value) => JSON.stringify(value)).join("\n");
async function put(relative: string, value: unknown, lines = false) {
  const file = path.join(host.home, relative);
  await mkdir(path.dirname(file), { recursive: true });
  await writeFile(file, lines ? jsonl(value as unknown[]) : JSON.stringify(value));
  return file;
}

interface Fixture { provider: string; file: string; value: unknown; lines?: boolean; sidecars?: Array<[string, unknown, boolean?]> }
const fixtures: Fixture[] = [
  { provider: "qoder", file: ".qoder/projects/task/q.jsonl", lines: true, value: [
    { type: "workspace-directories", directories: [cwd] },
    { type: "user", sessionId: "q", timestamp: at, message: { role: "user", content: user } },
    { type: "assistant", sessionId: "q", timestamp: later, message: { role: "assistant", content: answer } },
  ] },
  { provider: "codebuddy", file: ".codebuddy/projects/task/cb.jsonl", lines: true, value: [
    { type: "message", sessionId: "cb", cwd, role: "user", content: user, timestamp: at },
    { type: "message", sessionId: "cb", role: "assistant", content: [{ type: "output_text", text: answer }], timestamp: later },
  ] },
  { provider: "hermes", file: ".hermes/sessions/session_h.json", value: { session_id: "h", cwd, session_start: at, messages: [{ role: "user", content: user, timestamp: at }, { role: "assistant", content: answer, timestamp: later }] } },
  ...["pi", "omp", "prime-agent", "openclaw"].map((provider): Fixture => ({
    provider, file: ({ pi: ".pi/agent/sessions/p.jsonl", omp: ".omp/agent/sessions/p.jsonl", "prime-agent": ".prime/agent/sessions/p.jsonl", openclaw: ".openclaw/agents/main/sessions/p.jsonl" })[provider]!, lines: true,
    value: [{ type: "session", id: "p", cwd, timestamp: at }, { type: "message", message: { role: "user", content: user }, timestamp: at }, { type: "message", message: { role: "assistant", content: answer }, timestamp: later }],
  })),
  { provider: "cursor", file: ".cursor/projects/task/agent-transcripts/cu/cu.jsonl", lines: true, value: [{ role: "user", message: { content: user }, timestamp: at }, { role: "assistant", message: { content: answer }, timestamp: later }], sidecars: [[".cursor/chats/hash/cu/meta.json", { agentId: "cu", cwd, title: "Cursor session" }]] },
  { provider: "antigravity", file: ".gemini/antigravity-cli/brain/ag/.system_generated/logs/transcript.jsonl", lines: true, value: [{ source: "USER_EXPLICIT", type: "USER_INPUT", content: `<USER_REQUEST>${user}</USER_REQUEST>`, created_at: at }, { source: "MODEL", type: "PLANNER_RESPONSE", content: answer, created_at: later }], sidecars: [[".gemini/antigravity-cli/history.jsonl", [{ conversationId: "ag", workspace: cwd, timestamp: at }], true]] },
  { provider: "rovo", file: ".rovodev/sessions/r/metadata.json", value: { workspace_path: cwd, created_at: at }, sidecars: [[".rovodev/sessions/r/session_context.json", { messages: [{ role: "user", content: user, timestamp: at }, { role: "assistant", content: answer, timestamp: later }] }]] },
  { provider: "copilot", file: ".copilot/session-state/co/events.jsonl", lines: true, value: [{ type: "session.start", data: { sessionId: "co", context: { cwd }, startTime: at } }, { type: "user.message", data: { content: user }, timestamp: at }, { type: "assistant.message", data: { content: answer }, timestamp: later }] },
  { provider: "grok", file: ".grok/sessions/gr/summary.json", value: { info: { id: "gr", cwd }, created_at: at }, sidecars: [[".grok/sessions/gr/chat_history.jsonl", [{ type: "user", content: user, timestamp: at }, { type: "assistant", content: answer, timestamp: later }], true]] },
  { provider: "devin", file: ".local/share/devin/cli/transcripts/de.json", value: { session_id: "de", working_directory: cwd, steps: [{ source: "user", message: user, timestamp: at }, { source: "agent", message: { content: answer }, timestamp: later }] } },
  { provider: "droid", file: ".factory/sessions/dr.jsonl", lines: true, value: [{ type: "session_start", id: "dr", cwd, timestamp: at }, { type: "message", role: "user", content: user, timestamp: at }, { type: "message", role: "assistant", message: { content: answer }, timestamp: later }] },
  { provider: "cline", file: ".cline/data/sessions/cl/cl.json", value: { session_id: "cl", cwd, started_at: at }, sidecars: [[".cline/data/sessions/cl/cl.messages.json", { messages: [{ role: "user", content: user, ts: at }, { role: "assistant", content: answer, ts: later }] }]] },
  { provider: "kimi", file: ".kimi-code/sessions/work/session_ki/state.json", value: { title: "Kimi session", createdAt: at }, sidecars: [[".kimi-code/session_index.jsonl", [{ sessionId: "session_ki", workDir: cwd }], true], [".kimi-code/sessions/work/session_ki/agents/main/wire.jsonl", [{ type: "context.append_message", message: { role: "user", origin: { kind: "user" }, content: user } }, { type: "context.append_loop_event", event: { type: "content.part", part: { type: "text", text: answer } } }, { type: "context.append_loop_event", event: { type: "step.end" } }], true]] },
  { provider: "muse", file: ".local/share/muse/sessions/mu/session.jsonl", lines: true, value: [{ payload_type: "runtime.session.metadata", payload: { record: { workspace_root: cwd } }, recorded_at: 1793527200000000 }, { payload_type: "runtime.user_intent.accepted", payload: { refill_blocks: [{ text: user }] }, recorded_at: 1793527200000000 }, { payload_type: "runtime.session", payload: { event: { kind: "assistant_message_committed", text: answer } }, recorded_at: 1793527260000000 }] },
  { provider: "jcode", file: ".jcode/sessions/session_j.json", value: { id: "j", working_dir: cwd, messages: [{ role: "user", content: user, timestamp: at }, { role: "assistant", content: answer, timestamp: later }] } },
  { provider: "kiro", file: ".kiro/sessions/cli/00000000-0000-0000-0000-000000000001.json", value: { cwd, created_at: at }, sidecars: [[".kiro/sessions/cli/00000000-0000-0000-0000-000000000001.jsonl", [{ kind: "Prompt", data: { meta: { timestamp: at }, content: [{ kind: "text", data: user }] } }, { kind: "AssistantMessage", data: { content: [{ kind: "text", data: answer }] } }], true]] },
];

describe("additional file session readers", () => {
  beforeEach(async () => {
    for (const name of ["HERMES_HOME", "PI_CODING_AGENT_DIR", "OMP_CODING_AGENT_DIR", "PRIME_AGENT_SESSION_DIR", "PRIME_AGENT_CODING_AGENT_SESSION_DIR", "PRIME_AGENT_CODING_AGENT_DIR", "COPILOT_HOME", "GROK_HOME", "OPENCLAW_STATE_DIR", "DEVIN_HOME", "CLINE_SESSION_DATA_DIR", "KIMI_CODE_HOME", "XDG_DATA_HOME", "JCODE_HOME", "KIRO_HOME"]) vi.stubEnv(name, "");
    host.home = await mkdtemp(path.join(tmpdir(), "kanvibe-extra-history-")); });
  afterEach(async () => { await rm(host.home, { recursive: true, force: true }); vi.unstubAllEnvs(); });
  for (const fixture of fixtures) {
    it(`reads ${fixture.provider} list, content, pagination and worktree isolation`, async () => {
      const file = await put(fixture.file, fixture.value, fixture.lines);
      for (const [name, value, lines] of fixture.sidecars ?? []) await put(name, value, lines);
      const results = await readAdditionalFileSessions({ worktreePath: cwd, repoPath: cwd });
      const source = results.find((result) => result.provider === fixture.provider);
      expect(source?.sessions).toHaveLength(1);
      const session = source!.sessions[0];
      expect(session.matchedPath).toBe(cwd);
      expect(session.firstUserPrompt).toBe(user);
      const context = { worktreePath: cwd, repoPath: cwd };
      const detail = await readAdditionalFileSessionDetail(context, session.provider, session.id, file, null, 1);
      expect(detail?.messages).toHaveLength(1);
      expect(detail?.nextCursor).toBe("1");
      const second = await readAdditionalFileSessionDetail(context, session.provider, session.id, file, "1", 1);
      expect([detail!.messages[0].fullText, second!.messages[0].fullText].sort()).toEqual([answer, user].sort());
      const filtered = await readAdditionalFileSessionDetail({ ...context, roles: ["user"], query: "explain" }, session.provider, session.id, file);
      expect(filtered?.messages.map((message) => message.fullText)).toEqual([user]);
      expect(await readAdditionalFileSessionDetail(context, session.provider, "wrong-id", file)).toBeNull();
      expect(await readAdditionalFileSessionDetail({ ...context, worktreePath: "/other" }, session.provider, session.id, file)).toBeNull();
    });
  }
  it("rejects source references outside provider roots and keeps broken providers isolated", async () => {
    await put(".hermes/sessions/session_broken.json", null);
    const outside = await put("outside.json", { cwd, session_id: "secret", messages: [{ role: "user", content: "secret" }] });
    expect(await readAdditionalFileSessionDetail({ worktreePath: cwd, repoPath: cwd }, "hermes", "secret", outside)).toBeNull();
    const results = await readAdditionalFileSessions({ worktreePath: cwd, repoPath: cwd });
    expect(results).toHaveLength(19);
    expect(results.find((result) => result.provider === "hermes")?.sessions).toEqual([]);
  });
  it("keeps readable files when another transcript is malformed and rejects symlink references", async () => {
    const fixture = fixtures.find((entry) => entry.provider === "hermes")!;
    await put(fixture.file, fixture.value);
    const broken = path.join(host.home, ".hermes/sessions/session_bad.json");
    await writeFile(broken, "{broken");
    const outside = await put("outside.json", { cwd, session_id: "secret", messages: [{ role: "user", content: "private" }] });
    const link = path.join(host.home, ".hermes/sessions/session_link.json");
    await symlink(outside, link);
    const result = (await readAdditionalFileSessions({ worktreePath: cwd, repoPath: cwd })).find((entry) => entry.provider === "hermes");
    expect(result?.sessions.map((session) => session.id)).toEqual(["h"]);
    expect(result?.reason).toContain("could not be read");
    expect(await readAdditionalFileSessionDetail({ worktreePath: cwd, repoPath: cwd }, "hermes", "secret", link)).toBeNull();
  });
  it("classifies OMP-written transcripts in the Pi directory without duplicating them", async () => {
    await put(".pi/agent/sessions/omp.jsonl", [{ type: "session", id: "omp-id", cwd }, { type: "title", v: 1, title: "OMP" }, { type: "message", message: { role: "user", content: user } }], true);
    const result = await readAdditionalFileSessions({ worktreePath: cwd, repoPath: cwd });
    expect(result.find((entry) => entry.provider === "pi")?.sessions).toEqual([]);
    expect(result.find((entry) => entry.provider === "omp")?.sessions.map((session) => session.id)).toEqual(["omp-id"]);
  });
  it("reads Devin cwd and visibility from the sibling read-only database and observes updates", async () => {
    const fixture = fixtures.find((entry) => entry.provider === "devin")!;
    await put(fixture.file, { ...(fixture.value as object), working_directory: undefined });
    const db = path.join(host.home, ".local/share/devin/cli/sessions.db");
    await promisify(execFile)("python3", ["-c", "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('CREATE TABLE sessions (id TEXT, working_directory TEXT, title TEXT, hidden INTEGER)'); c.execute('INSERT INTO sessions VALUES (?,?,?,?)', ('de',sys.argv[2],'DB title',0)); c.commit(); c.close()", db, cwd]);
    const context = { worktreePath: cwd, repoPath: cwd };
    const first = (await readAdditionalFileSessions(context)).find((entry) => entry.provider === "devin");
    expect(first?.sessions[0].title).toBe("DB title");
    expect(first?.sessions[0].matchedPath).toBe(cwd);
    await promisify(execFile)("python3", ["-c", "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('UPDATE sessions SET hidden=1'); c.commit(); c.close()", db]);
    expect((await readAdditionalFileSessions(context)).find((entry) => entry.provider === "devin")?.sessions).toEqual([]);
  });
  it("does not guess an Antigravity workspace when metadata conflicts", async () => {
    const fixture = fixtures.find((entry) => entry.provider === "antigravity")!;
    await put(fixture.file, fixture.value, true);
    await put(".gemini/antigravity-cli/history.jsonl", [{ conversationId: "ag", workspace: cwd, timestamp: at }, { conversationId: "ag", workspace: "/other", timestamp: at }], true);
    expect((await readAdditionalFileSessions({ worktreePath: cwd, repoPath: cwd })).find((entry) => entry.provider === "antigravity")?.sessions).toEqual([]);
  });

  it("excludes another worktree's graph transcript using metadata before reading its body", async () => {
    const file = await put(".pi/agent/sessions/other.jsonl", [{ type: "session", id: "other", cwd: "/other" }, { type: "message", message: { role: "user", content: "large body".repeat(100_000) } }], true);
    vi.mocked(readTextFile).mockClear();
    const result = await readAdditionalFileSessions({ worktreePath: cwd, repoPath: cwd });
    expect(result.find((entry) => entry.provider === "pi")?.sessions).toEqual([]);
    expect(vi.mocked(readTextFile).mock.calls.some(([requested]) => requested === file)).toBe(false);
  });

  it("paginates Kimi turns newest first when every message has no timestamp", async () => {
    const fixture = fixtures.find((entry) => entry.provider === "kimi")!;
    const file = await put(fixture.file, fixture.value);
    await put(".kimi-code/session_index.jsonl", [{ sessionId: "session_ki", workDir: cwd }], true);
    const records = ["first", "second"].flatMap((turn) => [
      { type: "context.append_message", message: { role: "user", origin: { kind: "user" }, content: `${turn} user` } },
      { type: "context.append_loop_event", event: { type: "content.part", part: { type: "text", text: `${turn} assistant` } } },
      { type: "context.append_loop_event", event: { type: "step.end" } },
    ]);
    await put(".kimi-code/sessions/work/session_ki/agents/main/wire.jsonl", records, true);
    const context = { worktreePath: cwd, repoPath: cwd };
    const first = await readAdditionalFileSessionDetail(context, "kimi", "session_ki", file, null, 2);
    const second = await readAdditionalFileSessionDetail(context, "kimi", "session_ki", file, first!.nextCursor, 2);
    expect(first?.messages.map((message) => message.fullText)).toEqual(["second assistant", "second user"]);
    expect(second?.messages.map((message) => message.fullText)).toEqual(["first assistant", "first user"]);
    expect(second?.nextCursor).toBeNull();
  });
  it("preserves Kiro user, assistant and tool interleaving across pages with mixed timestamps", async () => {
    const fixture = fixtures.find((entry) => entry.provider === "kiro")!;
    const file = await put(fixture.file, fixture.value);
    await put(`${fixture.file}l`, [
      { kind: "Prompt", data: { meta: { timestamp: at }, content: [{ kind: "text", data: "first user" }] } },
      { kind: "AssistantMessage", data: { content: [{ kind: "text", data: "first assistant" }] } },
      { kind: "ToolResults", data: { content: [{ kind: "text", data: "tool result" }] } },
      { kind: "Prompt", data: { meta: { timestamp: later }, content: [{ kind: "text", data: "second user" }] } },
      { kind: "AssistantMessage", data: { content: [{ kind: "text", data: "second assistant" }] } },
    ], true);
    const context = { worktreePath: cwd, repoPath: cwd };
    const first = await readAdditionalFileSessionDetail(context, "kiro", "00000000-0000-0000-0000-000000000001", file, null, 3);
    const second = await readAdditionalFileSessionDetail(context, "kiro", "00000000-0000-0000-0000-000000000001", file, first!.nextCursor, 3);
    expect(first?.messages.map((message) => message.fullText)).toEqual(["second assistant", "second user", "tool result"]);
    expect(second?.messages.map((message) => message.fullText)).toEqual(["first assistant", "first user"]);
  });

});
