/* eslint-disable @typescript-eslint/no-require-imports */
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { execFileSync } = require("node:child_process");

const DEFAULT_FIXTURE_REPOSITORY_URL = "https://github.com/rookedsysc/kanvibe-qa-fixture.git";
const DEFAULT_BASE_BRANCH = "main";

function execGit(args, cwd) {
  return execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

function sanitizePathSegment(value) {
  const sanitized = String(value || "qa-run")
    .trim()
    .replace(/[^a-zA-Z0-9._-]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return sanitized || "qa-run";
}

function expandHome(value) {
  if (!value) return value;
  if (value === "~") return os.homedir();
  if (value.startsWith(`~${path.sep}`) || value.startsWith("~/")) {
    return path.join(os.homedir(), value.slice(2));
  }
  return value;
}

function resolveFixtureRepositoryUrl(env = process.env) {
  return env.KANVIBE_QA_FIXTURE_REPO_URL || DEFAULT_FIXTURE_REPOSITORY_URL;
}

function resolveBaseBranch(env = process.env) {
  return env.KANVIBE_QA_FIXTURE_BASE_BRANCH || DEFAULT_BASE_BRANCH;
}

function resolveWorktreeRoot(run, options = {}) {
  const configured = options.worktreeRoot || process.env.KANVIBE_QA_WORKTREE_ROOT;
  if (configured) return path.resolve(expandHome(configured));
  return path.join(os.homedir(), ".kanvibe-qa", "worktrees");
}

function buildFixtureIdentity(run, options = {}) {
  const runId = sanitizePathSegment(run.runId);
  const branchName = `qa/video-proof-${runId}`;
  const branchPathSegment = branchName.replace(/[\\/]+/g, "-");
  const worktreeRoot = resolveWorktreeRoot(run, options);
  const runWorktreeRoot = path.join(worktreeRoot, runId);

  return {
    projectName: `KanVibe QA Custom Worktree ${runId}`,
    projectDir: path.join(run.runDir, "fixtures", `project-clone-${runId}`),
    branchName,
    worktreePath: path.join(runWorktreeRoot, branchPathSegment),
    worktreeRoot,
    runWorktreeRoot,
    runId,
  };
}

function createFixtureRepository(run, options = {}) {
  const repositoryUrl = options.repositoryUrl || resolveFixtureRepositoryUrl(process.env);
  const baseBranch = options.baseBranch || resolveBaseBranch(process.env);
  const identity = buildFixtureIdentity(run, options);

  fs.rmSync(identity.projectDir, { recursive: true, force: true });
  fs.rmSync(identity.runWorktreeRoot, { recursive: true, force: true });
  fs.mkdirSync(path.dirname(identity.projectDir), { recursive: true });
  fs.mkdirSync(identity.runWorktreeRoot, { recursive: true });

  execGit(["clone", "--branch", baseBranch, "--single-branch", repositoryUrl, identity.projectDir], process.cwd());
  execGit(["config", "user.email", "qa@kanvibe.local"], identity.projectDir);
  execGit(["config", "user.name", "KanVibe QA"], identity.projectDir);
  execGit(["worktree", "add", "-b", identity.branchName, identity.worktreePath, baseBranch], identity.projectDir);

  return {
    ...identity,
    repositoryUrl,
    baseBranch,
  };
}

module.exports = {
  DEFAULT_FIXTURE_REPOSITORY_URL,
  buildFixtureIdentity,
  createFixtureRepository,
  resolveFixtureRepositoryUrl,
  resolveWorktreeRoot,
  sanitizePathSegment,
};
