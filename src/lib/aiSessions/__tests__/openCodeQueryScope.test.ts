/** @vitest-environment node */
import Database from "better-sqlite3";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readOpenCodeLiveSessions, readOpenCodeSessions } from "@/lib/aiSessions/readOpenCodeSessions";

let fixture: Database.Database;
vi.mock("@/lib/sqliteConnectionPool", async (original) => ({
  ...await original<typeof import("@/lib/sqliteConnectionPool")>(),
  getSqliteConnection: () => fixture,
}));
const worktreePath = "/fixtures/repo_%/task";
const context = { worktreePath, repoPath: "/fixtures/repo_%" };
beforeEach(() => {
  fixture = new Database(":memory:");
  fixture.exec(`CREATE TABLE session (id TEXT, directory TEXT, title TEXT, time_created INTEGER, time_updated INTEGER, parent_id TEXT);
    CREATE TABLE message (id TEXT, data TEXT);
    CREATE TABLE part (session_id TEXT, message_id TEXT, data TEXT, time_created INTEGER);`);
});
afterEach(() => fixture.close());
function insert(id: string, directory: string, time: number, title = "Session") {
  fixture.prepare("INSERT INTO session VALUES (?, ?, ?, ?, ?, NULL)").run(id, directory, title, time, time);
}
describe("OpenCode query candidate limit", () => {
  it("filters worktree and descendants before limiting history and live candidates", async () => {
    insert("target", worktreePath, 1);
    insert("nested", worktreePath + "/nested", 2);
    for (let index = 0; index < 130; index++) insert(`other-${index}`, "/other/task", 100 + index);
    insert("prefix-neighbor", worktreePath + "-other", 1000);
    insert("wildcard-neighbor", "/fixtures/repoAX/task", 1000);
    expect((await readOpenCodeSessions(context)).sessions.map((row) => row.id)).toEqual(["nested", "target"]);
    expect((await readOpenCodeLiveSessions(context, { runningWindowMs: Date.now(), recentWindowMs: 1000 })).map((row) => row.sessionId)).toEqual(["nested", "target"]);
  });
  it("applies search before limiting candidates within one worktree", async () => {
    insert("old-match", worktreePath, 1, "needle");
    for (let index = 0; index < 130; index++) insert(`new-${index}`, worktreePath, 100 + index);
    expect((await readOpenCodeSessions({ ...context, query: "needle" })).sessions.map((row) => row.id)).toEqual(["old-match"]);
  });
});
