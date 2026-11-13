import { execFile, spawn, type ChildProcess } from "node:child_process";
import { createServer } from "node:http";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { tmpdir } from "node:os";
import { promisify } from "node:util";
import { describe, expect, it, vi } from "vitest";
import { buildShellKanvibeStatusUpdater } from "@/lib/shellHookScript";
import { createLocalShellEnvironment } from "@/lib/shellEnvironment";

const mocks = vi.hoisted(() => ({ readSessions: vi.fn() }));
vi.mock("@/lib/aiSessions/readClaudeSessions", () => ({ readClaudeSessions: mocks.readSessions }));
vi.mock("@/lib/aiSessions/readCodexSessions", () => ({ readCodexSessions: mocks.readSessions }));
const execFileAsync = promisify(execFile);

describe("실제 hook 프로세스의 채팅 세션 바인딩", () => {
  for (const provider of ["claude", "codex"] as const) {
    it(`${provider} 조상 PID와 시작 시각에 실제 stdin 세션을 연결한다`, async () => {
      const directory = await mkdtemp(path.join(tmpdir(), "kanvibe-hook-process-"));
      let child: ChildProcess | undefined;
      const server = createServer();
      try {
        await execFileAsync("git", ["init", "-q"], { cwd: directory });
        const { recordPaneChatSessionBinding, findPaneChatSessionBinding } = await import("../paneChatSessionBindings");
        const context = { taskId: `fixture-${provider}`, worktreePath: directory, repoPath: directory, sshHost: null };
        let settle!: () => void;
        const posted = new Promise<void>((resolve) => { settle = resolve; });
        server.on("request", async (request, response) => {
          const chunks: Buffer[] = [];
          for await (const chunk of request) chunks.push(Buffer.from(chunk));
          const input = JSON.parse(Buffer.concat(chunks).toString());
          await recordPaneChatSessionBinding(context, input.agentSession);
          response.end("{}");
          settle();
        });
        await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
        const address = server.address();
        if (!address || typeof address === "string") throw new Error("fixture server unavailable");
        const hook = path.join(directory, "hook.sh");
        await writeFile(hook, `TASK_ID="${context.taskId}"\nKANVIBE_URL="http://127.0.0.1:${address.port}"\n${buildShellKanvibeStatusUpdater("progress", provider)}`);
        const executable = path.join(directory, provider);
        await writeFile(executable, 'import subprocess,sys,time\nsubprocess.run(["bash",sys.argv[1]],input=sys.argv[2],text=True,check=True)\ntime.sleep(30)\n');
        child = spawn("python3", [executable, hook, JSON.stringify({ session_id: `session-${provider}`, cwd: directory, transcript_path: "/ignored/client-path.jsonl" })], {
          cwd: directory, env: createLocalShellEnvironment(), stdio: "ignore",
        });
        await Promise.race([posted, new Promise<never>((_, reject) => setTimeout(() => reject(new Error("hook metadata timeout")), 10_000).unref())]);
        const start = (await execFileAsync("ps", ["-ww", "-p", String(child.pid), "-o", "lstart="])).stdout.trim().replace(/\s+/g, " ");
        mocks.readSessions.mockResolvedValue({ sessions: [{ id: `session-${provider}`, sourceRef: path.join(directory, "admitted.jsonl") }] });
        expect(await findPaneChatSessionBinding({ taskId: context.taskId, provider, providerPid: child.pid!, processStartTime: start })).toEqual({ sessionId: `session-${provider}`, sourceRef: path.join(directory, "admitted.jsonl") });
        expect(JSON.parse(await readFile(path.join(directory, ".kanvibe", "status.json"), "utf8"))).toMatchObject({ status: "progress" });
      } finally {
        if (child && child.exitCode === null) {
          const exited = new Promise<void>((resolve) => child!.once("exit", () => resolve()));
          child.kill("SIGTERM");
          await exited;
        }
        await new Promise<void>((resolve) => server.close(() => resolve()));
        await rm(directory, { recursive: true, force: true });
      }
    }, 15_000);
  }
});
