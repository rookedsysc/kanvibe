import { execFile } from "child_process";
import { mkdir, mkdtemp, rm } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { promisify } from "util";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readSqliteAgentSessions, readSqliteAgentSessionDetail } from "@/lib/aiSessions/readSqliteAgentSessions";

const execute = promisify(execFile);
const host = vi.hoisted(() => ({ home: "" }));
vi.mock("@/lib/hostFileAccess", async (load) => ({
  ...await load<typeof import("@/lib/hostFileAccess")>(),
  getHomeDirectory: async () => host.home,
}));

async function fixture(relativePath: string, sql: string) {
  const filename = path.join(host.home, relativePath);
  await mkdir(path.dirname(filename), { recursive: true });
  await execute("python3", ["-c", "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.executescript(sys.argv[2]); c.close()", filename, sql]);
  return filename;
}

const sessionSchema = `CREATE TABLE session_v2 (id TEXT PRIMARY KEY,directory TEXT,title TEXT,time_created INTEGER,time_updated INTEGER);
CREATE TABLE session_message (session_id TEXT,type TEXT,data TEXT,time_created INTEGER,seq INTEGER);`;
const zcodeSchema = `CREATE TABLE session (id TEXT PRIMARY KEY,directory TEXT,title TEXT,time_created INTEGER,time_updated INTEGER);
CREATE TABLE message (id TEXT PRIMARY KEY,session_id TEXT,data TEXT,time_created INTEGER,sequence INTEGER);
CREATE TABLE part (id TEXT PRIMARY KEY,message_id TEXT,session_id TEXT,data TEXT,time_created INTEGER,sequence INTEGER);`;
const context = { worktreePath: "/workspace/task", repoPath: "/workspace" };

describe("SQLite agent History", () => {
  beforeEach(async () => { host.home = await mkdtemp(path.join(tmpdir(), "kanvibe-sqlite-history-")); });
  afterEach(async () => { await rm(host.home, { recursive: true, force: true }); });

  it("reads OpenCode 2 from all matching database names and keeps task boundaries", async () => {
    const db = await fixture(".local/share/opencode/opencode-beta.db", sessionSchema + `
INSERT INTO session_v2 VALUES ('s','/workspace/task','Beta',1760000000000,1760000002000),('foreign','/workspace/task-other','Other',1760000000000,1760000003000);
INSERT INTO session_message VALUES ('s','user','{"text":["question","second line"]}',1760000000000,1),('s','assistant','{"content":[{"type":"text","text":"answer"}]}',1760000002000,2);`);
    const sources = await readSqliteAgentSessions(context);
    expect(sources.find((source) => source.provider === "opencode2")?.sessions).toMatchObject([
      { id: "s", provider: "opencode2", title: "Beta", firstUserPrompt: "question second line", messageCount: 2, sourceRef: db },
    ]);
    const firstPage = await readSqliteAgentSessionDetail(context, "opencode2", "s", db, null, 1);
    expect(firstPage?.messages.map((message) => message.fullText)).toEqual(["answer"]);
    expect(firstPage?.nextCursor).toBe("1");
    const secondPage = await readSqliteAgentSessionDetail(context, "opencode2", "s", db, "1", 1);
    expect(secondPage?.messages.map((message) => message.fullText)).toEqual(["question\nsecond line"]);
    expect(secondPage?.nextCursor).toBeNull();
    expect(await readSqliteAgentSessionDetail(context, "opencode2", "foreign", db)).toBeNull();
  });

  it("searches full OpenCode 2 conversations and applies role filters before pagination", async () => {
    await fixture(".local/share/opencode/opencode.db", sessionSchema + `
INSERT INTO session_v2 VALUES ('s','/workspace/task',NULL,1760000000000,1760000002000);
INSERT INTO session_message VALUES ('s','user','{"text":"first"}',1760000000000,1),('s','assistant','{"text":"needle answer"}',1760000002000,2);`);
    const sources = await readSqliteAgentSessions({ ...context, query: "NEEDLE" });
    expect(sources[0].sessions.map((session) => session.id)).toEqual(["s"]);
    const detail = await readSqliteAgentSessionDetail({ ...context, query: "needle", roles: ["assistant"] }, "opencode2", "s", null, null, 1);
    expect(detail?.messages.map((message) => message.fullText)).toEqual(["needle answer"]);
  });

  it("respects ZCode transcript visibility and sequence ordering", async () => {
    const db = await fixture(".zcode/cli/db/db.sqlite", zcodeSchema + `
INSERT INTO session VALUES ('s','/workspace/task',NULL,1760000000000,1760000003000);
INSERT INTO message VALUES ('m1','s','{"role":"user"}',1760000003000,1),('m2','s','{"role":"assistant"}',1760000001000,2),('hidden','s','{"role":"assistant","semantics":{"transcriptVisibility":"hidden"}}',1760000002000,3);
INSERT INTO part VALUES ('p1','m1','s','{"type":"text","text":"first prompt"}',1760000003000,1),('p2','m2','s','{"type":"text","text":"second reply"}',1760000001000,2),('p3','hidden','s','{"type":"text","text":"secret needle"}',1760000002000,3);`);
    const sources = await readSqliteAgentSessions(context);
    expect(sources.find((source) => source.provider === "zcode")?.sessions).toMatchObject([{ id: "s", messageCount: 2, firstUserPrompt: "first prompt" }]);
    const detail = await readSqliteAgentSessionDetail(context, "zcode", "s", db);
    expect(detail?.messages.map((message) => message.fullText)).toEqual(["second reply", "first prompt"]);
    const search = await readSqliteAgentSessions({ ...context, query: "secret needle" });
    expect(search.find((source) => source.provider === "zcode")?.sessions).toEqual([]);
  });

  it("ignores an arbitrary sourceRef even when its schema and task match", async () => {
    const db = await fixture("outside.db", sessionSchema + "INSERT INTO session_v2 VALUES ('s','/workspace/task','private',1760000000000,1760000000000);");
    expect(await readSqliteAgentSessionDetail(context, "opencode2", "s", db)).toBeNull();
    expect(await readSqliteAgentSessionDetail(context, "zcode", "s", db)).toBeNull();
  });

  it("keeps a readable database when another install has a broken schema", async () => {
    await fixture(".local/share/opencode/opencode.db", "CREATE TABLE session_v2 (id TEXT,directory TEXT,time_created INTEGER,time_updated INTEGER);");
    await fixture(".local/share/opencode/opencode-beta.db", sessionSchema + `
INSERT INTO session_v2 VALUES ('healthy','/workspace/task','Healthy',1760000000000,1760000002000);
INSERT INTO session_message VALUES ('healthy','user','{"text":"hello"}',1760000000000,1);`);
    const sources = await readSqliteAgentSessions(context);
    expect(sources[0]).toMatchObject({ available: true, sessions: [{ id: "healthy" }] });
    expect(sources[0].reason).toContain("schema is unreadable");
  });

  it("reads legacy ZCode schemas without sequence and preserves tool/reasoning roles", async () => {
    await fixture(".zcode/cli/db/db.sqlite", zcodeSchema.replaceAll(",sequence INTEGER", "") + `
INSERT INTO session VALUES ('s','/workspace/task','Tools',1760000000000,1760000003000);
INSERT INTO message VALUES ('m','s','{"role":"assistant"}',1760000001000);
INSERT INTO part VALUES ('p','m','s','{"type":"reasoning","text":"thinking"}',1760000001000),('t','m','s','{"type":"tool","tool":"read","state":{"output":"file body"}}',1760000002000);`);
    const detail = await readSqliteAgentSessionDetail({ ...context, roles: ["tool"] }, "zcode", "s");
    expect(detail?.messages).toMatchObject([{ role: "tool", fullText: "read: file body" }]);
  });

  it("reports absent stores without creating databases", async () => {
    const sources = await readSqliteAgentSessions(context);
    expect(sources).toMatchObject([{ provider: "opencode2", available: false }, { provider: "zcode", available: false }]);
  });
});
