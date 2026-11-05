// @vitest-environment node
import { afterEach, describe, expect, it } from "vitest";
import { mkdtemp, mkdir, rm, writeFile, realpath } from "fs/promises";
import { tmpdir } from "os";
import { join } from "path";
import { execFileSync, spawnSync } from "child_process";
import { getDefaultBranch, listBranches, listWorktrees, resolvePathForShell, scanGitRepos, validateGitRepo } from "../gitOperations";
import { buildTmuxUserClipboardDirectiveTestCommand, buildTmuxUserClipboardPreferenceCommand, createWorktreeWithSession, removeWorktreeAndBranch } from "../worktree";
import { SessionType } from "@/entities/KanbanTask";
import { listSubdirectories } from "@/desktop/main/services/projectService";

const fixtures: string[] = [];
afterEach(async () => { await Promise.all(fixtures.splice(0).map((directory) => rm(directory, { recursive: true, force: true }))); });
async function fixture() { const root = await mkdtemp(join(tmpdir(), "kanvibe-literal-")); fixtures.push(root); return realpath(root); }

describe("리터럴 셸 경로와 tmux 검사", () => {
  it("Git 조회와 worktree 생성/삭제에서 특수 경로를 그대로 사용한다", async () => {
    const root = await fixture();
    const repo = join(root, 'repo$ABSENT$(printf substituted)`printf tick`"quote\'');
    await mkdir(repo);
    execFileSync("git", ["init", "-b", "main", repo]);
    execFileSync("git", ["-C", repo, "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "--allow-empty", "-m", "fixture"]);
    expect(await validateGitRepo(repo)).toBe(true);
    expect(await getDefaultBranch(repo)).toBe("main");
    expect(await listBranches(repo, null, { refresh: false })).toContain("main");
    expect(await listWorktrees(repo)).toEqual([expect.objectContaining({ path: repo, branch: "main" })]);
    expect(await scanGitRepos(root)).toContain(repo);
    const created = await createWorktreeWithSession(repo, "feature/literal", "main", SessionType.TERMINAL);
    expect(await listWorktrees(repo)).toContainEqual(expect.objectContaining({ path: created.worktreePath }));
    await removeWorktreeAndBranch(repo, "feature/literal", null, { worktreePath: created.worktreePath, throwOnError: true });
    expect(await listWorktrees(repo)).toHaveLength(1);
  });

  it("폴더 탐색에서 명령 치환을 포함한 디렉터리 이름을 유지한다", async () => {
    const directory = join(await fixture(), '$(printf SUBSTITUTED)$ABSENT`printf tick`"quote\'');
    await mkdir(join(directory, "child"), { recursive: true });
    expect(await listSubdirectories(directory)).toEqual(["child"]);
  });

  it("원격 홈만 확장하고 뒤의 셸 문자는 리터럴로 유지한다", () => {
    const suffix = '/$ABSENT$(printf nope)`printf nope`"\'';
    expect(execFileSync("sh", ["-c", `printf '%s' ${resolvePathForShell(`~${suffix}`, "fixture")}`], { env: { HOME: "/fixture/home" }, encoding: "utf8" })).toBe(`/fixture/home${suffix}`);
  });

  it("grep 오류를 unknown으로 유지하고 설정이 없는 경우만 absent로 판정한다", async () => {
    const directory = await fixture();
    await writeFile(join(directory, ".tmux.conf"), "# fixture\n");
    for (const status of [0, 1, 2]) {
      const stub = `grep() { return ${status}; }; `;
      const env = { HOME: directory, XDG_CONFIG_HOME: join(directory, "config") };
      expect(spawnSync("sh", ["-c", stub + buildTmuxUserClipboardDirectiveTestCommand()], { env }).status).toBe(status);
      const preference = spawnSync("sh", ["-c", stub + buildTmuxUserClipboardPreferenceCommand()], { env, encoding: "utf8" });
      expect(preference.status).toBe(status === 2 ? 2 : 0);
      expect(preference.stdout).toBe(status === 0 ? "kanvibe-user-set-clipboard" : "");
    }
  });
});
