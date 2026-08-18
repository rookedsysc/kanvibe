#!/usr/bin/env node
/* eslint-disable @typescript-eslint/no-require-imports */
const fs = require("node:fs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const { createQaRun, writeReport } = require("../lib/report.cjs");
const { launchKanVibeElectron } = require("../lib/launchElectron.cjs");

const SCOPE = "실행중 AI 세션 추적: 보드 카드 배지, focus 후 세션 팝오버, 태스크 상세 dock 패널, 실행중 서브태스크 집계, 세션 클릭 시 tmux window 전환";

/**
 * QA 전용 tmux 소켓.
 * 앱은 plain `tmux`를 실행하므로 PATH 앞에 shim을 두어 이 소켓으로 유도한다.
 * 운영자의 기본 소켓과 완전히 분리되므로 실행이 중간에 죽어도 사용자 세션 목록이 더러워지지 않는다.
 */
const QA_TMUX_SOCKET = "kanvibe-qa";

/** 실행중으로 보이게 하려면 세션 기록이 이 창 안에 있어야 한다 */
const RUNNING_WINDOW_MS = 90_000;

/** 종료를 기다리다 리포트도 못 쓰고 매달리느니, 이만큼 기다린 뒤 정리를 이어간다 */
const APP_CLOSE_TIMEOUT_MS = 15_000;

function parseArgs(argv) {
  const args = {};
  for (let index = 2; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--run-dir") args.runDir = argv[++index];
    else if (arg === "--run-id") args.runId = argv[++index];
    else if (arg === "--output-root") args.outputRoot = argv[++index];
  }
  return args;
}

function gitValue(args) {
  try {
    return execFileSync("git", args, { encoding: "utf8" }).trim();
  } catch {
    return null;
  }
}

function resolveTmuxBinary() {
  return execFileSync("bash", ["-lc", "command -v tmux"], { encoding: "utf8" }).trim();
}

function qaTmux(args, options = {}) {
  return execFileSync(resolveTmuxBinary(), ["-L", QA_TMUX_SOCKET, ...args], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    ...options,
  }).trim();
}

function listQaTmuxSessions() {
  try {
    return qaTmux(["list-sessions", "-F", "#{session_name}"]).split("\n").filter(Boolean);
  } catch {
    return [];
  }
}

function createTmuxShim(binDir) {
  fs.mkdirSync(binDir, { recursive: true });
  const shimPath = path.join(binDir, "tmux");
  fs.writeFileSync(
    shimPath,
    `#!/usr/bin/env bash\nexec ${resolveTmuxBinary()} -L ${QA_TMUX_SOCKET} "$@"\n`,
    "utf8",
  );
  fs.chmodSync(shimPath, 0o755);
  return shimPath;
}

/**
 * QA 소켓 위의 세션을 모두 지운다.
 *
 * 이 소켓에는 PATH shim을 거친 호출만 닿고 운영자의 tmux는 기본 소켓에 있으므로, 격리 경계는
 * 세션 이름이 아니라 소켓 이름이다. 앱은 세션 이름을 프로젝트와 브랜치로 짓기 때문에
 * 이름 접두사로 가르면 정리 대상이 하나도 남지 않고, 그 잔여가 다음 실행의 첫 검사를 막는다.
 */
function cleanUpQaTmuxSessions() {
  for (const sessionName of listQaTmuxSessions()) {
    try {
      qaTmux(["kill-session", "-t", sessionName]);
    } catch {
      // 이미 사라진 세션은 정리 대상이 아니다.
    }
  }
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

/**
 * 실행중 판정과 서브태스크 집계를 결정적으로 검증하기 위한 세션 기록.
 * 최근 파일과 오래된 파일을 함께 심어, 창 안에 든 것만 세는지까지 확인한다.
 */
function createLiveSessionFixtures(fakeHome, worktreePath) {
  const claudeProjectDir = path.join(fakeHome, ".claude", "projects", claudeProjectDirectoryName(worktreePath));
  const claudeSessionId = "qa-live-claude-session";

  writeJsonLines(path.join(claudeProjectDir, `${claudeSessionId}.jsonl`), [
    { type: "user", sessionId: claudeSessionId, cwd: worktreePath, message: { role: "user", content: [{ type: "text", text: "QA live session prompt" }] } },
  ], 2_000);

  writeJsonLines(path.join(claudeProjectDir, claudeSessionId, "subagents", "agent-qa-running.jsonl"), [
    { agentId: "qa-running", isSidechain: true, message: { role: "user", content: [{ type: "text", text: "QA 서브에이전트: 코드베이스 조사" }] } },
  ], 3_000);

  writeJsonLines(path.join(claudeProjectDir, claudeSessionId, "subagents", "agent-qa-finished.jsonl"), [
    { agentId: "qa-finished", isSidechain: true, message: { role: "user", content: [{ type: "text", text: "QA 서브에이전트: 이미 끝난 작업" }] } },
  ], RUNNING_WINDOW_MS + 120_000);

  const codexSessionsDir = path.join(fakeHome, ".codex", "sessions", "2026", "08", "10");
  const sessionMeta = (payload) => ({ type: "session_meta", payload });

  writeJsonLines(path.join(codexSessionsDir, "rollout-qa-parent.jsonl"), [
    sessionMeta({ id: "qa-codex-parent", cwd: worktreePath, source: "cli", thread_source: "user" }),
  ], 4_000);

  writeJsonLines(path.join(codexSessionsDir, "rollout-qa-child.jsonl"), [
    sessionMeta({
      id: "qa-codex-child",
      cwd: worktreePath,
      parent_thread_id: "qa-codex-parent",
      thread_source: "subagent",
      agent_nickname: "Fermat",
    }),
  ], 5_000);

  return { claudeSessionId, claudeProjectDir, codexSessionsDir };
}

/**
 * Electron이 예기치 않게 죽으면 `page.evaluate`는 거부되지 않고 영원히 매달린다.
 * QA가 조용히 멈추는 대신 실패하도록 모든 렌더러 호출에 상한을 건다.
 */
function withTimeout(promise, timeoutMs, label) {
  let timeoutId;
  const timeout = new Promise((_, reject) => {
    timeoutId = setTimeout(() => reject(new Error(`${label} 시간 초과 (${timeoutMs}ms)`)), timeoutMs);
  });
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timeoutId));
}

async function invokeDesktop(page, namespace, method, ...args) {
  return withTimeout(
    page.evaluate(
      async ({ namespace, method, args }) => window.kanvibeDesktop.invoke(namespace, method, args),
      { namespace, method, args },
    ),
    60_000,
    `${namespace}.${method}`,
  );
}

async function takeScreenshot(page, run, label, screenshots) {
  const fileName = `${String(screenshots.length + 1).padStart(2, "0")}-${label}.png`;
  const shotPath = path.join(run.screenshotsDir, fileName);
  await page.screenshot({ path: shotPath, fullPage: true });
  screenshots.push({ label, path: shotPath });
  const videoPauseMs = Number.parseInt(process.env.KANVIBE_QA_VIDEO_STEP_PAUSE_MS || "900", 10);
  if (Number.isFinite(videoPauseMs) && videoPauseMs > 0) {
    await page.waitForTimeout(videoPauseMs);
  }
}

async function setStepOverlay(page, text) {
  await page.evaluate((message) => {
    let overlay = document.getElementById("kanvibe-qa-step-overlay");
    if (!overlay) {
      overlay = document.createElement("div");
      overlay.id = "kanvibe-qa-step-overlay";
      overlay.style.cssText = [
        "position:fixed", "top:16px", "left:50%", "transform:translateX(-50%)",
        "z-index:9999", "max-width:1080px", "padding:10px 14px", "border-radius:999px",
        "border:1px solid rgba(90,141,255,0.75)", "background:rgba(8,12,24,0.92)",
        "color:#f8fafc", "font:600 16px/1.35 ui-sans-serif, system-ui, sans-serif",
        "box-shadow:0 10px 30px rgba(0,0,0,0.35)", "pointer-events:none",
      ].join(";");
      document.body.appendChild(overlay);
    }
    overlay.textContent = message;
  }, text);
}

async function dismissUpdateDialogIfPresent(page) {
  try {
    await page.evaluate(() => {
      const button = document.querySelector("button[aria-label*='업데이트'][aria-label*='닫기']");
      if (button instanceof HTMLButtonElement) button.click();
    });
    await page.locator("[data-terminal-focus-blocker][data-shortcut-capture]")
      .waitFor({ state: "detached", timeout: 3000 }).catch(() => {});
  } catch {
    // 릴리스 알림이 없는 실행에서는 닫을 대상이 없다.
  }
}

async function openTaskDetail(page, taskId) {
  await page.evaluate((id) => {
    window.location.hash = `#/ko/task/${id}`;
  }, taskId);
  await page.waitForTimeout(1500);
  await dismissUpdateDialogIfPresent(page);
}

async function openBoard(page) {
  await page.evaluate(() => {
    window.location.hash = "#/ko";
  });
  await page.waitForTimeout(1200);
  await dismissUpdateDialogIfPresent(page);
}

function findTaskTmuxSession(sessionName) {
  return listQaTmuxSessions().find((name) => name === sessionName) || null;
}

/**
 * 태스크 tmux 세션에 에이전트 이름을 가진 프로세스를 띄운다.
 *
 * 앱이 attach한 pane에 send-keys로 밀어 넣으면 그 셸을 대체하게 되어 터미널 연결이 흔들리므로,
 * 별도 window를 만들어 거기서 실행한다. 바이너리를 복사해 이름만 바꾸는 방법은 uutils coreutils 같은
 * 멀티콜 바이너리가 argv[0]으로 동작을 고르기 때문에 쓸 수 없어, argv[0]만 바꿔 실행한다.
 */
function startFakeAgentWindow(sessionName, workingDir, agentName) {
  return qaTmux([
    "new-window", "-d", "-t", sessionName, "-c", workingDir, "-P", "-F", "#{window_id}",
    `bash -c 'exec -a ${agentName} /usr/bin/sleep 600'`,
  ]);
}

function readActiveWindowId(sessionName) {
  return qaTmux(["display-message", "-p", "-t", sessionName, "#{window_id}"]);
}

async function main() {
  const args = parseArgs(process.argv);
  const run = createQaRun({
    rootDir: process.cwd(),
    outputRoot: args.outputRoot,
    runId: args.runId,
    runDir: args.runDir,
  });

  const checks = [];
  const screenshots = [];
  const errors = [];
  const notes = [];
  const fakeHome = path.join(run.runDir, "fake-home");
  const shimDir = path.join(run.runDir, "bin");
  const previousHome = process.env.HOME;
  const previousPath = process.env.PATH;
  let app;
  let page;
  let fixtureRepoDir;
  let seededTask;
  let taskSessionName = null;
  let agentWindowId = null;
  let isExpectedShutdown = false;

  const check = async (name, fn) => {
    try {
      const detail = await fn();
      checks.push({ name, ok: true, detail: detail || "ok" });
      return detail;
    } catch (error) {
      const detail = error instanceof Error ? (error.stack || error.message) : String(error);
      checks.push({ name, ok: false, detail });
      throw error;
    }
  };

  try {
    await check("QA 전용 tmux 소켓을 비우고 shim을 건다", async () => {
      const leftovers = listQaTmuxSessions();
      cleanUpQaTmuxSessions();
      createTmuxShim(shimDir);
      return `socket=${QA_TMUX_SOCKET}, 정리한 잔여 세션=${leftovers.length === 0 ? "없음" : leftovers.join(",")}, shim=${path.join(shimDir, "tmux")}`;
    });

    fixtureRepoDir = path.join(run.runDir, "fixtures", "live-session-repo");
    fs.mkdirSync(fixtureRepoDir, { recursive: true });
    execFileSync("git", ["init", "-b", "main"], { cwd: fixtureRepoDir, stdio: "pipe" });
    execFileSync("git", ["config", "user.email", "qa@kanvibe.local"], { cwd: fixtureRepoDir, stdio: "pipe" });
    execFileSync("git", ["config", "user.name", "KanVibe QA"], { cwd: fixtureRepoDir, stdio: "pipe" });
    fs.writeFileSync(path.join(fixtureRepoDir, "README.md"), "# KanVibe live session QA fixture\n", "utf8");
    execFileSync("git", ["add", "README.md"], { cwd: fixtureRepoDir, stdio: "pipe" });
    execFileSync("git", ["commit", "-m", "Initial fixture commit"], { cwd: fixtureRepoDir, stdio: "pipe" });

    process.env.HOME = fakeHome;
    process.env.PATH = `${shimDir}${path.delimiter}${previousPath}`;
    fs.mkdirSync(fakeHome, { recursive: true });

    const launched = await launchKanVibeElectron({
      rootDir: process.cwd(),
      outputDir: run.runDir,
      appDataDir: path.join(run.runDir, "app-data"),
    });
    app = launched.app;
    page = launched.page;

    app.process().on("exit", (code, signal) => {
      if (!isExpectedShutdown) errors.push(`electron exited: code=${code ?? "null"} signal=${signal ?? "null"}`);
    });
    page.on("console", (message) => {
      if (["error", "warning"].includes(message.type())) errors.push(`${message.type()}: ${message.text()}`);
    });
    page.on("pageerror", (error) => errors.push(`pageerror: ${error.message}`));

    await page.waitForLoadState("domcontentloaded");
    await dismissUpdateDialogIfPresent(page);

    await check("픽스처 프로젝트와 tmux 세션 태스크를 만든다", async () => {
      const projectResult = await invokeDesktop(page, "project", "registerProject", "KanVibe Live Session QA", fixtureRepoDir);
      if (!projectResult.success || !projectResult.project) {
        throw new Error(projectResult.error || "project registration failed");
      }

      seededTask = await invokeDesktop(page, "kanban", "createTask", {
        title: "QA 실행중 세션 추적",
        description: "Electron QA: 실행중 AI 세션과 서브태스크 추적.",
        branchName: "feat/qa-live-sessions",
        baseBranch: "main",
        sessionType: "tmux",
        projectId: projectResult.project.id,
      });
      if (!seededTask?.id) throw new Error("task creation failed");

      const task = await invokeDesktop(page, "kanban", "getTaskById", seededTask.id);
      if (!task?.worktreePath) throw new Error(`task worktreePath가 없다: ${JSON.stringify(task)}`);
      if (!task.sessionName) throw new Error(`task sessionName이 없다: ${JSON.stringify(task)}`);

      seededTask = task;
      taskSessionName = task.sessionName;
      return `task=${task.id}, worktree=${task.worktreePath}, session=${task.sessionName}`;
    });

    await check("태스크 상세를 열면 tmux 세션이 QA 소켓에 생성된다", async () => {
      await openTaskDetail(page, seededTask.id);
      await page.getByTestId("terminal-tab-bar").waitFor({ state: "visible", timeout: 30000 });

      for (let attempt = 0; attempt < 20 && !findTaskTmuxSession(taskSessionName); attempt += 1) {
        await page.waitForTimeout(500);
      }

      if (!findTaskTmuxSession(taskSessionName)) {
        throw new Error(`QA 소켓에서 태스크 세션을 찾지 못했다: ${taskSessionName} (있는 세션: ${listQaTmuxSessions().join(",") || "없음"})`);
      }
      return `QA 소켓 세션=${listQaTmuxSessions().join(",")}`;
    });

    await check("실행중 세션 기록 픽스처를 심는다", async () => {
      const fixtures = createLiveSessionFixtures(fakeHome, seededTask.worktreePath);
      agentWindowId = startFakeAgentWindow(taskSessionName, seededTask.worktreePath, "claude");
      await page.waitForTimeout(2500);
      const panes = qaTmux(["list-panes", "-a", "-F", "#{session_name}|#{pane_current_command}|#{pane_current_path}"]);
      if (!panes.includes("claude")) {
        throw new Error(`tmux pane에 claude가 보이지 않는다:\n${panes}`);
      }
      return `claude session=${fixtures.claudeSessionId}, agentWindow=${agentWindowId}, panes=${panes.replace(/\n/g, " / ")}`;
    });

    await openBoard(page);
    await setStepOverlay(page, "1/4 보드: tmux 배지 옆에 실행중 에이전트 배지가 뜬다");

    await check("보드 카드에 실행중 에이전트 배지가 보인다", async () => {
      await page.locator("[data-testid='task-card-running-agents']").first()
        .waitFor({ state: "visible", timeout: 20000 });
      return "task-card-running-agents 배지 표시";
    });
    await takeScreenshot(page, run, "board-running-agent-badge", screenshots);

    await setStepOverlay(page, "2/4 보드: 카드에 focus를 올리면 잠시 뒤 세션 패널이 열린다");
    await check("카드 focus 후 세션 팝오버가 열리고 실행중 서브태스크를 보여준다", async () => {
      await page.locator(`[data-kanban-task-id='${seededTask.id}']`).first().focus();
      const popover = page.locator("[data-testid='task-card-live-session-popover']");
      await popover.waitFor({ state: "visible", timeout: 20000 });
      await page.locator("[data-testid='live-ai-session-claude']").first().waitFor({ state: "visible", timeout: 20000 });

      const popoverText = await popover.evaluate((node) => node.textContent || "");
      if (!popoverText.includes("코드베이스 조사")) {
        throw new Error(`실행중 서브에이전트 이름이 보이지 않는다: ${popoverText}`);
      }
      if (popoverText.includes("이미 끝난 작업")) {
        throw new Error(`오래된 서브에이전트가 실행중으로 집계됐다: ${popoverText}`);
      }
      return `popover=${popoverText.replace(/\s+/g, " ").trim()}`;
    });
    await takeScreenshot(page, run, "board-live-session-popover", screenshots);

    await setStepOverlay(page, "3/4 상세: dock의 실행중 세션 패널에서 claude/codex와 서브태스크 확인");
    await check("태스크 상세 dock 패널이 실행중 세션과 서브태스크를 보여준다", async () => {
      await openTaskDetail(page, seededTask.id);
      await page.getByRole("button", { name: "실행중 세션" }).first().click({ timeout: 20000 });
      await page.locator("[data-testid='live-ai-session-panel']").waitFor({ state: "visible", timeout: 20000 });
      await page.locator("[data-testid='live-ai-session-claude']").waitFor({ state: "visible", timeout: 20000 });

      const panelText = await page.locator("[data-testid='live-ai-session-panel']")
        .evaluate((node) => node.textContent || "");
      if (!panelText.includes("Fermat")) {
        throw new Error(`codex 서브에이전트(Fermat)가 보이지 않는다: ${panelText}`);
      }
      return `panel=${panelText.replace(/\s+/g, " ").trim()}`;
    });
    await takeScreenshot(page, run, "task-detail-live-session-panel", screenshots);

    await setStepOverlay(page, "4/4 세션 클릭: 해당 세션의 tmux window로 전환된다");
    await check("세션을 클릭하면 그 세션의 tmux window로 전환한다", async () => {
      const otherWindowId = qaTmux(["new-window", "-d", "-t", taskSessionName, "-P", "-F", "#{window_id}"]);
      qaTmux(["select-window", "-t", otherWindowId]);
      const beforeWindowId = readActiveWindowId(taskSessionName);
      if (beforeWindowId === agentWindowId) {
        throw new Error(`사전 조건 실패: 활성 window가 이미 에이전트 window다 (${beforeWindowId})`);
      }

      await page.locator("[data-testid='live-ai-session-claude'] button").first().click({ timeout: 20000 });
      await page.waitForTimeout(2000);

      const afterWindowId = readActiveWindowId(taskSessionName);
      if (afterWindowId !== agentWindowId) {
        throw new Error(`세션 클릭 후 에이전트 window로 전환되지 않았다: ${beforeWindowId} → ${afterWindowId} (기대=${agentWindowId})`);
      }
      return `activeWindow ${beforeWindowId} → ${afterWindowId} (에이전트 window)`;
    });
    await takeScreenshot(page, run, "session-click-switched-tmux-window", screenshots);

    await check("블로킹 콘솔 오류가 없다", async () => {
      const blocking = errors.filter((line) => !/favicon|DevTools|Electron Security Warning|Insecure Content-Security-Policy/i.test(line));
      if (blocking.length > 0) throw new Error(blocking.join("\n"));
      return "none";
    });
  } catch (error) {
    const detail = error instanceof Error ? (error.stack || error.message) : String(error);
    console.error(`[kanvibe-qa] live AI session flow failed:\n${detail}`);
  } finally {
    isExpectedShutdown = true;
    if (app) await withTimeout(app.close(), APP_CLOSE_TIMEOUT_MS, "Electron 종료").catch(() => {});
    try {
      cleanUpQaTmuxSessions();
      notes.push(`QA tmux 세션 정리 완료 (socket=${QA_TMUX_SOCKET}, 남은 세션=${listQaTmuxSessions().join(",") || "없음"})`);
    } catch (error) {
      notes.push(`QA tmux 정리 실패: ${error instanceof Error ? error.message : String(error)}`);
    }
    process.env.HOME = previousHome;
    process.env.PATH = previousPath;
  }

  const ok = checks.length > 0 && checks.every((item) => item.ok);
  notes.push(`fake HOME: ${fakeHome}`);
  notes.push("앱의 tmux 호출은 PATH shim으로 QA 전용 소켓에 묶여 있어 운영자의 기본 소켓과 분리된다.");
  if (fs.existsSync(run.videoPath)) notes.push("ffmpeg X11 화면 녹화 완료.");

  const result = {
    ok,
    scope: SCOPE,
    branch: gitValue(["branch", "--show-current"]),
    commit: gitValue(["rev-parse", "--short", "HEAD"]),
    checks,
    errors,
    screenshots,
    videoPath: fs.existsSync(run.videoPath) ? run.videoPath : null,
    notes,
  };

  writeReport(run, result);
  console.log(JSON.stringify({ ok, runDir: run.runDir, reportPath: run.reportPath, videoPath: run.videoPath }, null, 2));
  process.exit(ok ? 0 : 1);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
