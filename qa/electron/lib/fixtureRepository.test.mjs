import { createRequire } from "node:module";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { afterEach, describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);
const tempRoots = [];

function execGit(args, cwd) {
  return execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

function makeTempDir(prefix) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  tempRoots.push(dir);
  return dir;
}

function createSourceRepository() {
  const repoDir = makeTempDir("kanvibe-qa-source-");
  execGit(["init", "-b", "main"], repoDir);
  execGit(["config", "user.email", "qa@kanvibe.local"], repoDir);
  execGit(["config", "user.name", "KanVibe QA"], repoDir);
  fs.writeFileSync(path.join(repoDir, "README.md"), "# KanVibe QA fixture\n", "utf8");
  execGit(["add", "README.md"], repoDir);
  execGit(["commit", "-m", "fixture"], repoDir);
  return repoDir;
}

afterEach(() => {
  for (const root of tempRoots.splice(0)) {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

describe("QA fixture repository setup", () => {
  it("defaults to the public GitHub fixture repository so no local QA repo is required", () => {
    const { resolveFixtureRepositoryUrl } = require("./fixtureRepository.cjs");

    expect(resolveFixtureRepositoryUrl({})).toBe("https://github.com/rookedsysc/kanvibe-qa-fixture.git");
  });

  it("clones the fixture repo and creates the branch worktree under an explicit non-managed root", () => {
    const { createFixtureRepository } = require("./fixtureRepository.cjs");
    const sourceRepo = createSourceRepository();
    const runRoot = makeTempDir("kanvibe-qa-run-");
    const worktreeRoot = makeTempDir("kanvibe-qa-external-worktrees-");
    const run = {
      runId: "qa-custom-worktree-20260530T010203Z-12345",
      runDir: path.join(runRoot, "run"),
    };
    fs.mkdirSync(run.runDir, { recursive: true });

    const fixture = createFixtureRepository(run, {
      repositoryUrl: sourceRepo,
      worktreeRoot,
    });

    expect(fixture.projectName).toBe("KanVibe QA Custom Worktree qa-custom-worktree-20260530T010203Z-12345");
    expect(fixture.projectDir).toBe(path.join(run.runDir, "fixtures", "project-clone-qa-custom-worktree-20260530T010203Z-12345"));
    expect(fixture.worktreePath.startsWith(`${worktreeRoot}${path.sep}`)).toBe(true);
    expect(fixture.worktreePath).not.toContain("__worktrees");

    const worktrees = execGit(["worktree", "list", "--porcelain"], fixture.projectDir);
    expect(worktrees).toContain(fixture.worktreePath);
    expect(worktrees).toContain(`branch refs/heads/${fixture.branchName}`);
  });

  it("uses run-specific project names, clone directories, branches, and external worktree paths for parallel QA runs", () => {
    const { buildFixtureIdentity } = require("./fixtureRepository.cjs");
    const worktreeRoot = "/tmp/kanvibe-qa-worktrees";

    const first = buildFixtureIdentity({ runId: "parallel-run-a", runDir: "/tmp/run-a" }, { worktreeRoot });
    const second = buildFixtureIdentity({ runId: "parallel-run-b", runDir: "/tmp/run-b" }, { worktreeRoot });

    expect(first.projectName).not.toBe(second.projectName);
    expect(first.projectDir).not.toBe(second.projectDir);
    expect(first.branchName).not.toBe(second.branchName);
    expect(first.worktreePath).not.toBe(second.worktreePath);
    expect(first.worktreePath).toContain(`${path.sep}parallel-run-a${path.sep}`);
    expect(second.worktreePath).toContain(`${path.sep}parallel-run-b${path.sep}`);
  });
});
