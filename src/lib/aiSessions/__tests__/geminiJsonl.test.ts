import { mkdtemp, mkdir, writeFile, rm } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readGeminiSessions, readGeminiSessionDetail } from "@/lib/aiSessions/readGeminiSessions";

let home: string;
const worktreePath = "/workspace/gemini-jsonl";
beforeEach(async () => { home = await mkdtemp(path.join(tmpdir(), "gemini-jsonl-")); vi.stubEnv("HOME", home); });
afterEach(async () => { vi.unstubAllEnvs(); await rm(home, { recursive: true, force: true }); });
async function fixture(name: string, events: unknown[]) {
  const directory = path.join(home, ".gemini/tmp/project/chats");
  await mkdir(directory, { recursive: true });
  await writeFile(path.join(directory, "../.project_root"), worktreePath);
  const file = path.join(directory, name);
  await writeFile(file, events.map((event) => JSON.stringify(event)).join("\n") + "\n");
  return file;
}
const events = [
  { sessionId: "jsonl-session", startTime: "2026-11-01T00:00:00Z" },
  { type: "user", timestamp: "2026-11-01T00:01:00Z", content: "Initial question" },
  { type: "gemini", timestamp: "2026-11-01T00:02:00Z", content: "Unique answer" },
  { $set: { lastUpdated: "2026-11-01T00:03:00Z" } },
];
describe("Gemini JSONL History", () => {
  it("discovers JSONL alongside JSON and maps its project root", async () => {
    await fixture("session.jsonl", events);
    const result = await readGeminiSessions({ worktreePath, repoPath: worktreePath });
    expect(result.sessions).toMatchObject([{ id: "jsonl-session", title: "Initial question", messageCount: 2, matchedPath: worktreePath }]);
  });
  it("finds body matches and pages JSONL messages from newest to oldest", async () => {
    await fixture("session.jsonl", events);
    const context = { worktreePath, repoPath: worktreePath };
    expect((await readGeminiSessions({ ...context, query: "Unique answer" })).sessions).toHaveLength(1);
    const first = await readGeminiSessionDetail(context, "jsonl-session", undefined, undefined, 1);
    expect(first?.messages.map((message) => message.fullText)).toEqual(["Unique answer"]);
    const older = await readGeminiSessionDetail(context, "jsonl-session", undefined, first?.nextCursor, 1);
    expect(older?.messages.map((message) => message.fullText)).toEqual(["Initial question"]);
    expect(await readGeminiSessionDetail({ worktreePath: "/foreign", repoPath: "/foreign" }, "jsonl-session")).toBeNull();
  });
});
