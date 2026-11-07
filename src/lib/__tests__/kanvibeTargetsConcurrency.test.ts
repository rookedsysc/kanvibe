// @vitest-environment node
import { execFile } from "child_process";
import { mkdir, mkdtemp, readFile, rm, readdir, writeFile } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { promisify } from "util";
import { afterEach, describe, expect, it, vi } from "vitest";

const execFileAsync = promisify(execFile);
vi.mock("@/lib/gitOperations", () => ({
  execGit: async (command: string, _sshHost: string, options?: { remoteStdin?: string }) => {
    // macOS에서도 Linux CI에서 관측한 명령 인자 제한을 재현한다.
    if (Buffer.byteLength(command) >= 128 * 1024) {
      throw Object.assign(new Error("spawn E2BIG"), { code: "E2BIG" });
    }
    const execution = execFileAsync("sh", ["-c", command]);
    execution.child.stdin?.end(options?.remoteStdin ?? "");
    return (await execution).stdout;
  },
}));
import { upsertKanvibeHookTarget } from "@/lib/kanvibeProjectState";
import { updateTextFile } from "@/lib/hostFileAccess";

const fixtures: string[] = [];
async function fixture() {
  const repo = await mkdtemp(path.join(tmpdir(), "kanvibe-targets-'"));
  fixtures.push(repo);
  return repo;
}
afterEach(async () => {
  await Promise.all(fixtures.splice(0).map((repo) => rm(repo, { recursive: true, force: true })));
});

for (const mode of ["local", "remote", "mixed"] as const) {
  it(`preserves all concurrent distinct clients with ${mode} repository access`, async () => {
    const repo = await fixture();
    const targets = Array.from({ length: 8 }, (_, index) => ({ url: `http://client-${index}:19736`, taskId: `task-${index}` }));
    await Promise.all(targets.map((target, index) => upsertKanvibeHookTarget(
      repo, target, mode === "remote" || (mode === "mixed" && index % 2 === 0) ? "fixture-host" : undefined,
    )));
    const state = JSON.parse(await readFile(path.join(repo, ".kanvibe/targets.json"), "utf8"));
    expect(state.targets).toHaveLength(targets.length);
    expect(state.targets).toEqual(expect.arrayContaining(targets));
    expect(await readdir(path.join(repo, ".kanvibe"))).toEqual(["targets.json"]);
  });
}

describe("independent app processes", () => {
  it("keeps registrations made by two separate processes", async () => {
    const repo = await fixture();
    const modulePath = path.resolve("src/lib/kanvibeProjectState.ts");
    await Promise.all([0, 1].map((client) => execFileAsync(process.execPath, [
      "--import", "tsx", "-e",
      `const { upsertKanvibeHookTarget } = require(${JSON.stringify(modulePath)});
       Promise.all(Array.from({ length: 4 }, (_, i) => upsertKanvibeHookTarget(${JSON.stringify(repo)}, { url: 'http://client-${client}-' + i, taskId: 'task-' + i })));`,
    ])));
    const state = JSON.parse(await readFile(path.join(repo, ".kanvibe/targets.json"), "utf8"));
    expect(state.targets).toHaveLength(8);
  });
});

for (const host of [undefined, "fixture-host"]) {
  it(`releases the ${host ?? "local"} lock after update failure without replacing saved contents`, async () => {
    const repo = await fixture();
    const target = path.join(repo, "targets.json");
    await writeFile(target, "saved");
    await expect(updateTextFile(target, () => { throw new Error("update failed"); }, host)).rejects.toThrow("update failed");
    expect(await readFile(target, "utf8")).toBe("saved");
    await updateTextFile(target, (content) => content + " next", host);
    expect(await readFile(target, "utf8")).toBe("saved next");
    expect(await readdir(repo)).toEqual(["targets.json"]);
  });

  it(`does not interpret a ${host ?? "local"} read failure as an absent file`, async () => {
    const repo = await fixture();
    const target = path.join(repo, "targets.json");
    await mkdir(target);
    const update = vi.fn(() => "replacement");
    await expect(updateTextFile(target, update, host)).rejects.toThrow();
    expect(update).not.toHaveBeenCalled();
    expect(await readdir(repo)).toEqual(["targets.json"]);
  });

  it(`publishes only complete JSON while ${host ?? "local"} readers run during writes`, async () => {
    const repo = await fixture();
    const target = path.join(repo, ".kanvibe/targets.json");
    await upsertKanvibeHookTarget(repo, { url: "http://initial", taskId: "initial" }, host);
    let completed = false;
    let reads = 0;
    const observer = (async () => {
      while (!completed) {
        expect(JSON.parse(await readFile(target, "utf8")).schemaVersion).toBe(1);
        reads++;
      }
    })();
    try {
      await Promise.all(Array.from({ length: 4 }, (_, index) => upsertKanvibeHookTarget(repo, {
        url: `http://client-${index}`, taskId: "x".repeat(100_000),
      }, host)));
    } finally {
      completed = true;
      await observer;
    }
    expect(reads).toBeGreaterThan(0);
    expect(JSON.parse(await readFile(target, "utf8")).targets).toHaveLength(5);
  });
}
