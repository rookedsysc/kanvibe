import { execFile } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { promisify } from "node:util";
import { describe, expect, it } from "vitest";
import { buildShellKanvibeStatusUpdater } from "../shellHookScript";

const execFileAsync = promisify(execFile);

describe("hook chat metadata transport", () => {
  for (const { provider, oversized } of [{ provider: "claude", oversized: false }, { provider: "codex", oversized: false }, { provider: "claude", oversized: true }] as const) {
    it(`${provider} hook의 ${oversized ? "큰 stdin도 끝까지 비우면서 상태를 유지한다" : "실제 stdin과 sender PID를 기존 상태 본문에 함께 전송한다"}`, async () => {
      const directory = await mkdtemp(path.join(tmpdir(), "kanvibe-chat-hook-"));
      try {
        await execFileAsync("git", ["init", "-q"], { cwd: directory });
        const capture = path.join(directory, "payload.json");
        const script = [
          'TASK_ID="task-hook"',
          'KANVIBE_URL="http://127.0.0.1:1"',
          'curl() { while [ "$#" -gt 0 ]; do if [ "$1" = "-d" ]; then shift; printf "%s" "$1" > "$KANVIBE_QA_CAPTURE"; return; fi; shift; done; }',
          buildShellKanvibeStatusUpdater("progress", provider),
        ].join("\n");
        const payload = { session_id: "session-hook", cwd: directory, transcript_path: "/safe/한글.jsonl", prompt: oversized ? "큰 본문".repeat(100_000) : "줄1\n줄2" };
        const pid = await new Promise<number>((resolve, reject) => {
          const child = execFile("bash", ["-c", script], {
            cwd: directory,
            env: { ...process.env, KANVIBE_QA_CAPTURE: capture },
            timeout: 10_000,
          }, (error) => error ? reject(error) : resolve(child.pid!));
          child.stdin!.end(JSON.stringify(payload));
        });
        const posted = JSON.parse(await readFile(capture, "utf8"));
        expect(posted).toMatchObject({ taskId: "task-hook", status: "progress", agentSession: { provider, hookPid: pid } });
        const forwarded = Buffer.from(posted.agentSession.payloadBase64, "base64");
        if (oversized) expect(forwarded.byteLength).toBeLessThanOrEqual(64 * 1_024);
        else expect(JSON.parse(forwarded.toString("utf8"))).toEqual(payload);
      } finally {
        await rm(directory, { recursive: true, force: true });
      }
    });
  }
});
