import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ home: vi.fn(), exec: vi.fn() }));
vi.mock("@/lib/hostFileAccess", async (importOriginal) => ({ ...await importOriginal<typeof import("@/lib/hostFileAccess")>(), getHomeDirectory: mocks.home }));
vi.mock("@/lib/gitOperations", () => ({ execGit: mocks.exec }));
import { findPaneChatProcessSessionBinding, findPaneChatReportedSessionBinding, findPaneChatSessionBinding, recordPaneChatSessionBinding } from "../paneChatSessionBindings";

let directory: string;
beforeEach(async () => {
  directory = await mkdtemp(path.join(tmpdir(), "kanvibe-exact-session-"));
  mocks.home.mockResolvedValue(directory);
});
afterEach(async () => { await rm(directory, { recursive: true, force: true }); });

describe("exact Claude IDs through the real transcript reader", () => {
  it.each(["reported", "hook", "process"] as const)("resolves %s metadata when the session UUID is absent from message content", async (source) => {
    const cwd = path.join(directory, "repo");
    const sessionId = "2a366fc3-88b8-4e99-a2f4-6ca4af64d959";
    const projectDirectory = path.join(directory, ".claude", "projects", source === "process" ? "migrated-project-directory" : cwd.replaceAll(path.sep, "-").replaceAll("_", "-"));
    await mkdir(projectDirectory, { recursive: true });
    const sourceRef = path.join(projectDirectory, `${sessionId}.jsonl`);
    await writeFile(sourceRef, JSON.stringify({ type: "user", sessionId, cwd, timestamp: "2026-11-16T00:00:00Z", message: { role: "user", content: "A prompt with no identifier" } }) + "\n");
    const context = { taskId: directory, worktreePath: cwd, repoPath: cwd, sshHost: null };
    if (source === "process") {
      const processDirectory = path.join(directory, ".claude", "sessions");
      await mkdir(processDirectory, { recursive: true });
      await writeFile(path.join(processDirectory, "100.json"), JSON.stringify({ pid: 100, sessionId, cwd, kind: "interactive", procStart: "12345", pidDomain: "linux:machine:pid:[1]" }));
      mocks.exec.mockResolvedValue(`100 (claude) S ${Array(18).fill("0").join(" ")} 12345 0\nlinux:machine:pid:[1]\n`);
      expect(await findPaneChatProcessSessionBinding(context, { pid: 100, provider: "claude", processStartTime: "current" })).toEqual({ sessionId, sourceRef });
    } else if (source === "reported") {
      expect(await findPaneChatReportedSessionBinding(context, "claude", sessionId)).toEqual({ sessionId, sourceRef });
    } else {
      const started = "Mon Nov 16 08:00:00 2026";
      mocks.exec.mockResolvedValue(`100 1 ${started} claude claude\n300 100 ${started} bash bash hook.sh`);
      await recordPaneChatSessionBinding(context, { provider: "claude", hookPid: 300, payloadBase64: Buffer.from(JSON.stringify({ session_id: sessionId, cwd })).toString("base64") });
      expect(await findPaneChatSessionBinding({ taskId: directory, provider: "claude", providerPid: 100, processStartTime: started })).toMatchObject({ sessionId, sourceRef });
    }
  });
});
