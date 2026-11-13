// @vitest-environment node
import { execFile, spawn } from "child_process";
import { mkdir, mkdtemp, readFile, rm, readdir, rename, writeFile, utimes } from "fs/promises";
import { hostname, tmpdir } from "os";
import path from "path";
import { promisify } from "util";
import { afterEach, describe, expect, it, vi } from "vitest";

const execFileAsync = promisify(execFile);
const lockObservations = vi.hoisted(() => ({ onAttempt: undefined as ((filePath: string) => void) | undefined }));
vi.mock("fs/promises", async (importOriginal) => {
  const actual = await importOriginal<typeof import("fs/promises")>();
  return {
    ...actual,
    mkdir: async (...args: Parameters<typeof actual.mkdir>) => {
      try {
        return await actual.mkdir(...args);
      } finally {
        lockObservations.onAttempt?.(String(args[0]));
      }
    },
  };
});
vi.mock("@/lib/gitOperations", () => ({
  execGit: async (command: string, _sshHost: string, options?: { remoteStdin?: string; remoteStdinResponder?: { marker: string; respond: (payload: string) => Promise<string> } }) => {
    // macOS에서도 Linux CI에서 관측한 명령 인자 제한을 재현한다.
    if (Buffer.byteLength(command) >= 128 * 1024) {
      throw Object.assign(new Error("spawn E2BIG"), { code: "E2BIG" });
    }
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
import { upsertKanvibeHookTarget } from "@/lib/kanvibeProjectState";
import { updateTextFile } from "@/lib/hostFileAccess";

const fixtures: string[] = [];
async function fixture() {
  const repo = await mkdtemp(path.join(tmpdir(), "kanvibe-targets-'"));
  fixtures.push(repo);
  return repo;
}
afterEach(async () => {
  lockObservations.onAttempt = undefined;
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
  it(`does not publish or remove a successor after the ${host ?? "local"} lock is replaced`, async () => {
    const repo = await fixture();
    const target = path.join(repo, "targets.json");
    await writeFile(target, "saved");
    const marker = `owner-${process.pid}-successor-${Buffer.from(hostname()).toString("hex")}`;
    await expect(updateTextFile(target, async () => {
      await rename(`${target}.lock`, `${target}.previous-lock`);
      await mkdir(`${target}.lock/${marker}`, { recursive: true });
      return "replacement";
    }, host)).rejects.toThrow();
    expect(await readFile(target, "utf8")).toBe("saved");
    expect(await readdir(`${target}.lock`)).toEqual([marker]);
  });

  it(`recovers a proven dead ${host ?? "local"} owner and preserves saved contents`, async () => {
    const repo = await fixture();
    const target = path.join(repo, "targets.json");
    await writeFile(target, "saved");
    const exited = spawn(process.execPath, ["-e", ""], { stdio: "ignore" });
    const ownerPid = exited.pid!;
    await new Promise<void>((resolve) => exited.once("close", () => resolve()));
    await mkdir(`${target}.lock/owner-${ownerPid}-dead-${Buffer.from(hostname()).toString("hex")}`, { recursive: true });
    await updateTextFile(target, (content) => `${content} next`, host);
    expect(await readFile(target, "utf8")).toBe("saved next");
    expect(await readdir(repo)).toEqual(["targets.json"]);
  });

  it(`does not reclaim a ${host ?? "local"} owner from another host by its PID`, async () => {
    const repo = await fixture();
    const target = path.join(repo, "targets.json");
    await writeFile(target, "saved");
    const exited = spawn(process.execPath, ["-e", ""], { stdio: "ignore" });
    const ownerPid = exited.pid!;
    await new Promise<void>((resolve) => exited.once("close", () => resolve()));
    const marker = `owner-${ownerPid}-dead-${Buffer.from("another-host").toString("hex")}`;
    await mkdir(`${target}.lock/${marker}`, { recursive: true });
    const update = vi.fn(() => "replacement");
    await expect(updateTextFile(target, update, host)).rejects.toThrow(`${target}.lock`);
    expect(update).not.toHaveBeenCalled();
    expect(await readFile(target, "utf8")).toBe("saved");
    expect(await readdir(`${target}.lock`)).toEqual([marker]);
  });

  it(`preserves an ownerless ${host ?? "local"} lock and reports its path`, async () => {
    const repo = await fixture();
    const target = path.join(repo, "targets.json");
    await writeFile(target, "saved");
    await mkdir(`${target}.lock`);
    const old = new Date(Date.now() - 3_600_000);
    await utimes(`${target}.lock`, old, old);
    const update = vi.fn(() => "replacement");
    await expect(updateTextFile(target, update, host)).rejects.toThrow(`${target}.lock`);
    expect(update).not.toHaveBeenCalled();
    expect(await readFile(target, "utf8")).toBe("saved");
    expect(await readdir(`${target}.lock`)).toEqual([]);
  });

  it(`keeps an active async ${host ?? "local"} owner until its callback finishes`, async () => {
    const repo = await fixture();
    const target = path.join(repo, "targets.json");
    let release: () => void = () => {};
    let entered: () => void = () => {};
    const callbackEntered = new Promise<void>((resolve) => { entered = resolve; });
    const held = new Promise<void>((resolve) => { release = resolve; });
    const owner = updateTextFile(target, async (content) => { entered(); await held; return `${content}a`; }, host);
    await callbackEntered;
    const following = vi.fn((content: string) => `${content}b`);
    const attempted = new Promise<void>((resolve) => {
      lockObservations.onAttempt = (filePath) => { if (filePath === `${target}.lock`) resolve(); };
    });
    const waiter = updateTextFile(target, following);
    try {
      await attempted;
      expect(following).not.toHaveBeenCalled();
    } finally {
      release();
      lockObservations.onAttempt = undefined;
    }
    await Promise.all([owner, waiter]);
    expect(await readFile(target, "utf8")).toBe("ab");
    expect(following).toHaveBeenCalledTimes(1);
  });

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
