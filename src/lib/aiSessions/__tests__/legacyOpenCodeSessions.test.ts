import { execFile } from "child_process";
import { mkdir, mkdtemp, rm, writeFile } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { promisify } from "util";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readOpenCodeSessions, readOpenCodeSessionDetail } from "@/lib/aiSessions/readOpenCodeSessions";

vi.mock("@/lib/sqliteConnectionPool", () => ({ getSqliteConnection: () => null }));
let home: string;
const cwd = "/projects/legacy-task";
const context = { worktreePath: cwd, repoPath: "/projects" };
async function put(relative: string, value: unknown) {
  const file = path.join(home, ".local/share/opencode/storage", relative);
  await mkdir(path.dirname(file), { recursive: true });
  await writeFile(file, JSON.stringify(value));
  return file;
}
async function session(id: string, directory = cwd) {
  return put(`session/project/${id}.json`, { id, directory, title: "Legacy session", time: { created: 1793527200000, updated: 1793527260000 } });
}
async function message(id: string, messageId: string, role: string, content: unknown, created: number) {
  return put(`message/${id}/${messageId}.json`, { id: messageId, sessionID: id, role, content, time: { created } });
}

describe("legacy OpenCode History", () => {
  beforeEach(async () => { home = await mkdtemp(path.join(tmpdir(), "kanvibe-legacy-opencode-")); vi.stubEnv("HOME", home); vi.stubEnv("XDG_DATA_HOME", ""); });
  afterEach(async () => { vi.unstubAllEnvs(); await rm(home, { recursive: true, force: true }); });

  it("reads inline messages, orders files by creation time, and paginates with role/search filters", async () => {
    const file = await session("ses_legacy");
    await message("ses_legacy", "a", "assistant", [{ type: "text", text: "finished change" }], 1793527260000);
    await message("ses_legacy", "z", "user", [{ type: "text", text: "please change this" }], 1793527200000);
    await session("ses_other", "/other");
    const listed = await readOpenCodeSessions(context);
    expect(listed.available).toBe(true);
    expect(listed.sessions.map((entry) => entry.id)).toEqual(["ses_legacy"]);
    expect(listed.sessions[0]).toMatchObject({ sourceRef: file, firstUserPrompt: "please change this", messageCount: 2 });
    const detail = await readOpenCodeSessionDetail(context, "ses_legacy", file, null, 1);
    expect(detail?.messages.map((entry) => entry.fullText)).toEqual(["finished change"]);
    expect(detail?.nextCursor).toBe("1");
    expect((await readOpenCodeSessionDetail(context, "ses_legacy", file, "1", 1))?.messages.map((entry) => entry.fullText)).toEqual(["please change this"]);
    expect((await readOpenCodeSessionDetail({ ...context, roles: ["user"], query: "please" }, "ses_legacy", file))?.messages.map((entry) => entry.fullText)).toEqual(["please change this"]);
    expect(await readOpenCodeSessionDetail({ ...context, worktreePath: "/other" }, "ses_legacy", file)).toBeNull();
  });
  it("reads separate part files without inventing messages from generated summaries", async () => {
    const file = await session("ses_parts");
    await message("ses_parts", "msg_user", "user", undefined, 1793527200000);
    await put("part/msg_user/part_a.json", { id: "part_a", messageID: "msg_user", sessionID: "ses_parts", type: "text", text: "real prompt" });
    await put("message/ses_parts/msg_summary.json", { id: "msg_summary", role: "assistant", summary: { body: "generated summary" }, time: { created: 1793527260000 } });
    const detail = await readOpenCodeSessionDetail(context, "ses_parts", file);
    expect(detail?.messages.map((entry) => entry.fullText)).toEqual(["real prompt"]);
  });
  it("rejects outside source refs, malformed files and session identifiers that escape storage", async () => {
    const file = await session("ses_safe");
    await message("ses_safe", "msg", "user", "safe", 1793527200000);
    const outside = path.join(home, "outside.json");
    await writeFile(outside, JSON.stringify({ id: "ses_safe", directory: cwd }));
    expect(await readOpenCodeSessionDetail(context, "ses_safe", outside)).toBeNull();
    expect(await readOpenCodeSessionDetail(context, "../../outside", file)).toBeNull();
    await put("session/project/broken.json", null);
    expect((await readOpenCodeSessions(context)).sessions).toHaveLength(1);
  });
  it("prefers SQLite over a stale legacy file with the same session ID", async () => {
    const file = await session("ses_shared");
    await message("ses_shared", "legacy", "user", "stale prompt", 1793527200000);
    const db = path.join(home, ".local/share/opencode/opencode.db");
    await promisify(execFile)("python3", ["-c", `import sqlite3,sys,json
c=sqlite3.connect(sys.argv[1])
c.executescript("CREATE TABLE session(id TEXT,directory TEXT,title TEXT,time_created INTEGER,time_updated INTEGER); CREATE TABLE message(id TEXT,session_id TEXT,data TEXT); CREATE TABLE part(id TEXT,message_id TEXT,session_id TEXT,data TEXT,time_created INTEGER);")
c.execute("INSERT INTO session VALUES(?,?,?,?,?)",("ses_shared",sys.argv[2],"SQLite session",1793527200000,1793527260000))
c.execute("INSERT INTO message VALUES(?,?,?)",("msg_sql","ses_shared",json.dumps({"role":"user"})))
c.execute("INSERT INTO part VALUES(?,?,?,?,?)",("prt","msg_sql","ses_shared",json.dumps({"type":"text","text":"current prompt"}),1793527260000))
c.commit();c.close()`, db, cwd]);
    const listed = await readOpenCodeSessions(context);
    expect(listed.sessions).toHaveLength(1);
    expect(listed.sessions[0].firstUserPrompt).toBe("current prompt");
    expect((await readOpenCodeSessions({ ...context, query: "stale prompt" })).sessions).toEqual([]);
    expect((await readOpenCodeSessionDetail(context, "ses_shared", file))?.messages.map((entry) => entry.fullText)).toEqual(["current prompt"]);
    await promisify(execFile)("python3", ["-c", "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('DELETE FROM part'); c.execute('DELETE FROM message'); c.commit(); c.close()", db]);
    expect((await readOpenCodeSessionDetail(context, "ses_shared", file))?.messages).toEqual([]);
  });
});
