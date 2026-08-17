#!/usr/bin/env node
/* eslint-disable @typescript-eslint/no-require-imports */
/**
 * docs용 실행중 AI 세션 스크린샷 캡처 플로우.
 *
 * 앱이 실행하는 tmux를 PATH shim으로 전용 소켓에 묶는다. 그러지 않으면 캡처 시점의
 * tmux 서버 상태가 그대로 찍혀 운영자의 실제 에이전트 세션과 경로가 이미지에 남는다.
 * HOME도 실행 디렉터리 안으로 옮겨 실제 AI 히스토리가 화면에 들어오지 않게 한다.
 */
const fs = require("node:fs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const { launchKanVibeElectron } = require("../lib/launchElectron.cjs");

const ROOT_DIR = process.env.KANVIBE_ROOT_DIR || process.cwd();
const OUT_DIR = process.env.CAPTURE_OUT_DIR;
const BOARD_VIEWPORT = { width: 1680, height: 1040 };

const QA_TMUX_SOCKET = "kanvibe-docs";

/** 배지와 패널이 함께 보이도록 두 provider를 서로 다른 태스크에 붙인다 */
const AGENT_TASKS = [
  { project: "kanvibe", title: "feat/live-session-panel", agent: "claude", status: "progress" },
  { project: "techtaurant-be", title: "refactor/auth-detail", agent: "codex", status: "review" },
];

const PROJECT_FIXTURES = [
  { name: "kanvibe", owner: "rookedsysc" },
  { name: "techtaurant-be", owner: "rookedsysc" },
];

const FILLER_TASKS = [
  { project: "kanvibe", title: "fix/kanban-task-move", status: "done" },
  { project: "techtaurant-be", title: "feat/comment-thread", status: "todo" },
];

function execGit(args, cwd) {
  return execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

function resolveTmuxBinary() {
  return execFileSync("bash", ["-lc", "command -v tmux"], { encoding: "utf8" }).trim();
}

function docsTmux(args) {
  return execFileSync(resolveTmuxBinary(), ["-L", QA_TMUX_SOCKET, ...args], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

function listDocsTmuxSessions() {
  try {
    return docsTmux(["list-sessions", "-F", "#{session_name}"]).split("\n").filter(Boolean);
  } catch {
    return [];
  }
}

/** 앱은 plain `tmux`를 실행하므로 PATH 앞의 shim이 소켓을 결정한다 */
function createTmuxShim(binDir) {
  fs.mkdirSync(binDir, { recursive: true });
  const shimPath = path.join(binDir, "tmux");
  fs.writeFileSync(shimPath, `#!/usr/bin/env bash\nexec ${resolveTmuxBinary()} -L ${QA_TMUX_SOCKET} "$@"\n`, "utf8");
  fs.chmodSync(shimPath, 0o755);
  return shimPath;
}

function createLocalRepository(fixturesRoot, fixture) {
  const projectDir = path.join(fixturesRoot, fixture.name);
  fs.mkdirSync(projectDir, { recursive: true });
  execGit(["init", "--initial-branch=main"], projectDir);
  execGit(["config", "user.email", "docs@kanvibe.local"], projectDir);
  execGit(["config", "user.name", "KanVibe Docs"], projectDir);
  fs.writeFileSync(path.join(projectDir, "README.md"), `# ${fixture.name}\n`, "utf8");
  execGit(["add", "."], projectDir);
  execGit(["commit", "-m", "chore: init"], projectDir);
  if (fixture.owner) {
    execGit(["remote", "add", "origin", `https://github.com/${fixture.owner}/${fixture.name}.git`], projectDir);
  }
  return projectDir;
}

function writeJsonLines(filePath, values, ageMs = 0) {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, `${values.map((value) => JSON.stringify(value)).join("\n")}\n`, "utf8");
  const modifiedAt = new Date(Date.now() - ageMs);
  fs.utimesSync(filePath, modifiedAt, modifiedAt);
}

function claudeProjectDirectoryName(targetPath) {
  return path.resolve(targetPath).replaceAll(path.sep, "-").replaceAll("_", "-");
}

/** 화면에 실행중 세션과 서브태스크가 실제로 그려지도록 최근 활동 기록을 심는다 */
function seedClaudeSession(fakeHome, worktreePath) {
  const projectDir = path.join(fakeHome, ".claude", "projects", claudeProjectDirectoryName(worktreePath));
  const sessionId = "docs-live-claude";

  writeJsonLines(path.join(projectDir, `${sessionId}.jsonl`), [
    { type: "user", sessionId, cwd: worktreePath, message: { role: "user", content: [{ type: "text", text: "실행중 세션 패널 구현" }] } },
    { type: "assistant", sessionId, message: { role: "assistant", content: [{ type: "text", text: "서브에이전트 3개로 나눠 조사 중입니다" }] } },
  ], 2_000);

  const subagents = [
    ["docs-explore", "코드베이스에서 세션 리더 위치 조사"],
    ["docs-review", "실행중 판정 로직 리뷰"],
    ["docs-test", "실행중 판정 회귀 테스트 작성"],
  ];
  for (const [agentId, taskLabel] of subagents) {
    writeJsonLines(path.join(projectDir, sessionId, "subagents", `agent-${agentId}.jsonl`), [
      { agentId, isSidechain: true, message: { role: "user", content: [{ type: "text", text: taskLabel }] } },
    ], 4_000);
  }
}

function seedCodexSession(fakeHome, worktreePath) {
  const sessionsDir = path.join(fakeHome, ".codex", "sessions", "2026", "08", "10");
  const sessionMeta = (payload) => ({ type: "session_meta", payload });

  writeJsonLines(path.join(sessionsDir, "rollout-docs-parent.jsonl"), [
    sessionMeta({ id: "docs-codex-parent", cwd: worktreePath, source: "cli", thread_source: "user" }),
    { type: "response_item", payload: { type: "message", role: "user", content: [{ type: "input_text", text: "인증 상세 리팩터링" }] } },
    { type: "response_item", payload: { type: "message", role: "assistant", content: [{ type: "output_text", text: "토큰 검증 경로를 정리하는 중입니다" }] } },
  ], 3_000);
  writeJsonLines(path.join(sessionsDir, "rollout-docs-child.jsonl"), [
    sessionMeta({
      id: "docs-codex-child",
      cwd: worktreePath,
      parent_thread_id: "docs-codex-parent",
      thread_source: "subagent",
      agent_nickname: "Fermat",
    }),
  ], 5_000);
}

/**
 * pane이 에이전트 이름을 갖게 한다.
 * 바이너리를 복사해 이름만 바꾸면 멀티콜 바이너리가 argv[0]으로 동작을 골라 실행이 거부되므로
 * argv[0]만 바꿔 실행한다.
 */
function startAgentWindow(sessionName, workingDir, agentName) {
  return docsTmux([
    "new-window", "-d", "-t", sessionName, "-c", workingDir, "-P", "-F", "#{window_id}",
    `bash -c 'exec -a ${agentName} /usr/bin/sleep 900'`,
  ]);
}

async function invokeDesktop(page, namespace, method, ...args) {
  return page.evaluate(
    async ({ namespace, method, args }) => window.kanvibeDesktop.invoke(namespace, method, args),
    { namespace, method, args },
  );
}

async function captureRetinaScreenshot(app, shotPath) {
  const encodedPng = await app.evaluate(async ({ BrowserWindow }) => {
    const win = BrowserWindow.getAllWindows()[0];
    const image = await win.webContents.capturePage();
    return image.toPNG().toString("base64");
  });
  fs.writeFileSync(shotPath, Buffer.from(encodedPng, "base64"));
}

async function openTaskDetail(page, taskId) {
  await page.evaluate((id) => { window.location.hash = `#/ko/task/${id}`; }, taskId);
  await page.waitForTimeout(1500);
  await dismissDialogIfPresent(page);
}

async function waitForTmuxSession(page, sessionName) {
  for (let attempt = 0; attempt < 30; attempt += 1) {
    if (listDocsTmuxSessions().includes(sessionName)) return;
    await page.waitForTimeout(500);
  }
  throw new Error(`tmux 세션이 docs 소켓에 나타나지 않았다: ${sessionName} (있는 세션: ${listDocsTmuxSessions().join(",") || "없음"})`);
}

async function dismissDialogIfPresent(page) {
  try {
    await page.getByRole("button", { name: "닫기" }).first().click({ timeout: 2500 });
    await page.waitForTimeout(500);
  } catch {
    // 릴리스 노트 다이얼로그는 없을 수도 있다.
  }
}

async function main() {
  if (!OUT_DIR) throw new Error("CAPTURE_OUT_DIR is required");

  const fixturesRoot = path.join(OUT_DIR, "fixtures");
  const appDataDir = path.join(OUT_DIR, "app-data");
  const fakeHome = path.join(OUT_DIR, "fake-home");
  const shimDir = path.join(OUT_DIR, "bin");
  for (const dir of [fixturesRoot, appDataDir, fakeHome]) {
    fs.rmSync(dir, { recursive: true, force: true });
    fs.mkdirSync(dir, { recursive: true });
  }

  const foreign = listDocsTmuxSessions();
  if (foreign.length > 0) {
    throw new Error(`docs tmux 소켓에 기존 세션이 있다: ${foreign.join(", ")}`);
  }
  createTmuxShim(shimDir);

  const projectDirs = new Map();
  for (const fixture of PROJECT_FIXTURES) {
    projectDirs.set(fixture.name, createLocalRepository(fixturesRoot, fixture));
  }

  const previousHome = process.env.HOME;
  const previousPath = process.env.PATH;
  process.env.HOME = fakeHome;
  process.env.PATH = `${shimDir}${path.delimiter}${previousPath}`;

  const { app, page } = await launchKanVibeElectron({
    rootDir: ROOT_DIR,
    appDataDir,
    viewport: BOARD_VIEWPORT,
    timeoutMs: 60000,
    actionTimeoutMs: 30000,
  });

  try {
    await page.waitForLoadState("domcontentloaded");
    await page.waitForTimeout(2500);
    await dismissDialogIfPresent(page);

    const projectIds = new Map();
    for (const fixture of PROJECT_FIXTURES) {
      const result = await invokeDesktop(page, "project", "registerProject", fixture.name, projectDirs.get(fixture.name));
      if (!result?.success || !result.project) throw new Error(`register failed: ${fixture.name} ${result?.error}`);
      projectIds.set(fixture.name, result.project.id);
    }

    for (const task of FILLER_TASKS) {
      const created = await invokeDesktop(page, "kanban", "createTask", { title: task.title, description: "", projectId: projectIds.get(task.project) });
      if (task.status !== "todo" && created?.id) {
        await invokeDesktop(page, "kanban", "moveTaskToColumn", created.id, task.status, [created.id]);
      }
    }

    let heroTaskId = null;
    for (const fixture of AGENT_TASKS) {
      const created = await invokeDesktop(page, "kanban", "createTask", {
        title: fixture.title,
        description: "",
        branchName: fixture.title,
        baseBranch: "main",
        projectId: projectIds.get(fixture.project),
      });
      if (!created?.id) throw new Error(`task creation failed: ${fixture.title}`);
      if (fixture.status !== "todo") {
        await invokeDesktop(page, "kanban", "moveTaskToColumn", created.id, fixture.status, [created.id]);
      }

      const connected = await invokeDesktop(page, "kanban", "connectTerminalSession", created.id, "tmux");
      if (!connected?.sessionName || !connected?.worktreePath) {
        throw new Error(`connectTerminalSession failed: ${JSON.stringify(connected)}`);
      }

      // tmux 세션은 터미널 화면이 붙을 때 만들어지므로, 상세를 한 번 열어야 소켓에 세션이 생긴다.
      await openTaskDetail(page, created.id);
      await waitForTmuxSession(page, connected.sessionName);

      startAgentWindow(connected.sessionName, connected.worktreePath, fixture.agent);
      if (fixture.agent === "claude") {
        // 배지가 provider별 개수를 보여주므로 같은 에이전트를 하나 더 띄워 2개로 만든다
        startAgentWindow(connected.sessionName, connected.worktreePath, fixture.agent);
        seedClaudeSession(fakeHome, connected.worktreePath);
        heroTaskId = created.id;
      } else {
        seedCodexSession(fakeHome, connected.worktreePath);
      }
      console.log(`[capture] ${fixture.agent} on ${connected.sessionName} (${connected.worktreePath})`);
    }

    await page.evaluate(() => { window.location.hash = "#/ko"; });
    await page.waitForTimeout(1500);
    await page.reload();
    await page.waitForLoadState("domcontentloaded");
    await page.waitForTimeout(4000);
    await dismissDialogIfPresent(page);

    await page.locator("[data-testid='task-card-running-agents']").first().waitFor({ state: "visible", timeout: 30000 });
    await page.locator(`[data-kanban-task-id='${heroTaskId}']`).first().focus();
    await page.locator("[data-testid='task-card-live-session-popover']").waitFor({ state: "visible", timeout: 30000 });
    await page.locator("[data-testid='live-ai-session-claude']").first().waitFor({ state: "visible", timeout: 30000 });
    await page.waitForTimeout(1200);
    await captureRetinaScreenshot(app, path.join(OUT_DIR, "live-sessions-board.png"));

    await openTaskDetail(page, heroTaskId);
    await page.getByRole("button", { name: "실행중 세션" }).first().click({ timeout: 30000 });
    await page.locator("[data-testid='live-ai-session-panel']").waitFor({ state: "visible", timeout: 30000 });
    await page.locator("[data-testid='live-ai-subtask']").first().waitFor({ state: "visible", timeout: 30000 });
    await page.waitForTimeout(1200);
    await captureRetinaScreenshot(app, path.join(OUT_DIR, "live-sessions-panel.png"));

    console.log(JSON.stringify({ ok: true, outDir: OUT_DIR }));
  } catch (error) {
    console.error("[capture] electron output:\n" + (typeof app.output === "function" ? app.output() : ""));
    throw error;
  } finally {
    await app.close().catch(() => {});
    for (const sessionName of listDocsTmuxSessions()) {
      try {
        docsTmux(["kill-session", "-t", sessionName]);
      } catch {
        // 이미 사라진 세션은 정리 대상이 아니다.
      }
    }
    process.env.HOME = previousHome;
    process.env.PATH = previousPath;
  }
}

main().catch((error) => {
  console.error("[capture] failed:", error);
  process.exit(1);
});
