// @vitest-environment node
import { execFile } from "child_process";
import { mkdtemp, readFile, readdir, rm } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { promisify } from "util";
import { afterEach, expect, it, vi } from "vitest";
import { TaskPriority } from "@/entities/TaskPriority";

const execFileAsync = promisify(execFile);
vi.mock("@/lib/gitOperations", () => ({
  execGit: async (command: string, _sshHost: string, options?: { remoteStdin?: string; remoteStdinResponder?: { marker: string; respond: (payload: string) => Promise<string> } }) => {
    const execution = execFileAsync("sh", ["-c", command]);
    /** 실제 전송 경로처럼, 명령이 입력을 다 읽기 전에 끝나 생기는 EPIPE는 종료 코드로 판정한다 */
    execution.child.stdin?.on("error", () => undefined);
    let callbackError: unknown;
    if (options?.remoteStdinResponder) {
      const responder = options.remoteStdinResponder;
      let output = "";
      let replied = false;
      execution.child.stdout?.on("data", (chunk: string) => {
        output += chunk;
        const line = output.split("\n").find((value) => value.startsWith(`${responder.marker}\t`));
        if (replied || line === undefined || !output.includes(`${line}\n`)) return;
        replied = true;
        void responder.respond(line.slice(responder.marker.length + 1)).then(
          (reply) => execution.child.stdin?.end(reply),
          (error) => { callbackError = error; execution.child.stdin?.end(); },
        );
      });
    } else {
      execution.child.stdin?.end(options?.remoteStdin ?? "");
    }
    try {
      return (await execution).stdout.trim();
    } catch (error) {
      throw callbackError ?? error;
    }
  },
}));
import { writeKanvibeProjectColor, writeKanvibeProjectPriority } from "@/lib/kanvibeProjectState";

const fixtures: string[] = [];
afterEach(async () => {
  await Promise.all(fixtures.splice(0).map((repo) => rm(repo, { recursive: true, force: true })));
});

for (const mode of ["local", "remote", "mixed"] as const) {
  for (const priority of [TaskPriority.HIGH, null]) {
    it(`preserves concurrent color and ${priority ?? "cleared"} priority with ${mode} repository access`, async () => {
      const repo = await mkdtemp(path.join(tmpdir(), "kanvibe-project-'"));
      fixtures.push(repo);
      const colorHost = mode === "local" ? undefined : "fixture-host";
      const priorityHost = mode === "remote" ? "fixture-host" : undefined;
      await writeKanvibeProjectColor(repo, "#111111", colorHost);
      await writeKanvibeProjectPriority(repo, TaskPriority.LOW, priorityHost);

      await Promise.all([
        writeKanvibeProjectColor(repo, "#222222", colorHost),
        writeKanvibeProjectPriority(repo, priority, priorityHost),
      ]);

      const state = JSON.parse(await readFile(path.join(repo, ".kanvibe/project.json"), "utf8"));
      expect(state).toMatchObject({ projectColor: "#222222", priority });
      expect(await readdir(path.join(repo, ".kanvibe"))).toEqual(["project.json"]);
    });
  }
}
