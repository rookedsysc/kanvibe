/**
 * @vitest-environment node
 */
import { execFile } from "node:child_process";
import { access, mkdir, mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { promisify } from "node:util";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const execFileAsync = promisify(execFile);

const mocks = vi.hoisted(() => ({ execGit: vi.fn() }));

vi.mock("@/lib/gitOperations", () => ({ execGit: mocks.execGit }));

const { assertKanvibeStatePathsAreNotSymlinks, upsertKanvibeHookTarget } = await import("@/lib/kanvibeProjectState");

describe("KanVibe 상태 경로의 심볼릭 링크 거부", () => {
  let workspaceDir: string;
  let repoDir: string;
  let outsideDir: string;

  beforeEach(async () => {
    workspaceDir = await mkdtemp(path.join(tmpdir(), "kanvibe-symlink-"));
    repoDir = path.join(workspaceDir, "repo");
    outsideDir = path.join(workspaceDir, "outside");
    await mkdir(repoDir);
    await mkdir(outsideDir);
    mocks.execGit.mockReset();
    mocks.execGit.mockImplementation(async (command: string) => (await execFileAsync("sh", ["-c", command])).stdout);
  });

  afterEach(async () => {
    await rm(workspaceDir, { recursive: true, force: true });
  });

  it("링크가 없으면 통과한다", async () => {
    await mkdir(path.join(repoDir, ".kanvibe"));

    await expect(assertKanvibeStatePathsAreNotSymlinks(repoDir, ["task.sh"])).resolves.toBeUndefined();
    await expect(assertKanvibeStatePathsAreNotSymlinks(repoDir, ["task.sh"], "remote-host")).resolves.toBeUndefined();
  });

  it.each([[null], ["remote-host"]])("task.sh가 링크면 거부한다 (sshHost=%s)", async (sshHost) => {
    await mkdir(path.join(repoDir, ".kanvibe"));
    await writeFile(path.join(outsideDir, "victim"), "keep");
    await symlink(path.join(outsideDir, "victim"), path.join(repoDir, ".kanvibe", "task.sh"));

    await expect(assertKanvibeStatePathsAreNotSymlinks(repoDir, ["task.sh"], sshHost)).rejects.toThrow("심볼릭 링크");
  });

  it.each([[null], ["remote-host"]])(".kanvibe가 링크면 토큰을 그 대상에 남기지 않는다 (sshHost=%s)", async (sshHost) => {
    await symlink(outsideDir, path.join(repoDir, ".kanvibe"));

    const registration = upsertKanvibeHookTarget(repoDir, { url: "http://10.0.0.2:9736", taskId: "task", token: "secret" }, sshHost);

    await expect(registration).rejects.toThrow("심볼릭 링크");
    await expect(access(path.join(outsideDir, "targets.json"))).rejects.toThrow();
  });
});
