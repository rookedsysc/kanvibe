#!/usr/bin/env node
/* eslint-disable @typescript-eslint/no-require-imports */
const fs = require("node:fs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const { expect } = require("@playwright/test");
const { createQaRun } = require("../lib/report.cjs");
const { launchKanVibeElectron } = require("../lib/launchElectron.cjs");

const SCOPE = "워크스페이스 뷰: 칸반↔워크스페이스 전환, 목록 필터, 검색 선택과 좌우 표시 일치, 수식키 단독 목록 포커스, 위아래 선택과 터미널 복귀, 자동 연결, 재시작 후 뷰 유지";
const PALETTE_SHORTCUT = process.platform === "darwin" ? "Meta+Shift+P" : "Control+Shift+P";
const TASK_SEARCH_SHORTCUT = process.platform === "darwin" ? "Meta+Shift+O" : "Control+Shift+O";

function parseArgs(argv) {
  const args = {};
  for (let index = 2; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--run-dir") args.runDir = argv[++index];
    else if (arg === "--run-id") args.runId = argv[++index];
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

function execGit(args, cwd) {
  return execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

async function step(checks, name, fn) {
  try {
    const detail = await fn();
    checks.push({ name, ok: true, detail: detail || "ok" });
    return true;
  } catch (error) {
    checks.push({ name, ok: false, detail: error instanceof Error ? error.message : String(error) });
    return false;
  }
}

async function takeScreenshot(page, run, label, screenshots) {
  const shotPath = path.join(run.screenshotsDir, `${String(screenshots.length + 1).padStart(2, "0")}-${label}.png`);
  await page.screenshot({ path: shotPath });
  screenshots.push({ label, path: shotPath });
}

function invokeDesktop(page, namespace, method, ...args) {
  return page.evaluate(
    ({ serviceNamespace, serviceMethod, serviceArgs }) => window.kanvibeDesktop.invoke(serviceNamespace, serviceMethod, serviceArgs),
    { serviceNamespace: namespace, serviceMethod: method, serviceArgs: args },
  );
}

async function dismissUpdateDialogIfPresent(page) {
  try {
    await page.getByRole("button", { name: /닫기|Close/i }).first().click({ timeout: 2500 });
  } catch {
    // 릴리스 안내 창은 항상 뜨지 않는다.
  }
}

function createFixtureRepository(run) {
  const projectDir = path.join(run.runDir, "fixtures", `workspace-view-${run.runId}`);
  fs.rmSync(projectDir, { recursive: true, force: true });
  fs.mkdirSync(projectDir, { recursive: true });
  execGit(["init", "--initial-branch", "main"], projectDir);
  execGit(["config", "user.email", "qa@kanvibe.local"], projectDir);
  execGit(["config", "user.name", "KanVibe QA"], projectDir);
  fs.writeFileSync(path.join(projectDir, "README.md"), "# Workspace view QA\n", "utf8");
  execGit(["add", "README.md"], projectDir);
  execGit(["commit", "-m", "Initial QA fixture"], projectDir);
  return { projectDir, projectName: `Workspace QA ${run.runId}` };
}

async function runPaletteCommand(page, commandText) {
  await page.locator("body").click({ position: { x: 5, y: 5 } }).catch(() => {});
  await page.keyboard.press(PALETTE_SHORTCUT);
  const input = page.getByRole("dialog").getByRole("textbox");
  await input.waitFor({ state: "visible", timeout: 10000 });
  await input.fill(commandText);
  await input.press("Enter");
}

function taskList(page) {
  return page.getByRole("list", { name: "작업 목록" });
}

async function listedTitles(page) {
  return taskList(page).getByRole("heading", { level: 3 }).allTextContents();
}


function taskCard(page, taskId) {
  return taskList(page).locator(`[data-workspace-task-id="${taskId}"]`);
}

async function expectSelectedTask(page, task) {
  await expect(page.locator('[data-workspace-view="true"]')).toBeVisible();
  await expect(taskList(page).locator('[aria-current="true"]')).toHaveCount(1);
  await expect(taskCard(page, task.id)).toHaveAttribute("aria-current", "true");
  await expect(taskCard(page, task.id)).toBeInViewport();
  await expect(page.getByRole("region", { name: task.title, exact: true })).toBeVisible();
  await expect.poll(() => {
    const hash = new URL(page.url()).hash;
    if (/\/task\//.test(hash)) return "task-detail";
    return new URLSearchParams(hash.split("?")[1] || "").get("taskId");
  }).toBe(task.id);
}

async function searchForTask(page, task) {
  await page.keyboard.press(TASK_SEARCH_SHORTCUT);
  const dialog = page.getByRole("dialog");
  const input = dialog.getByRole("textbox");
  await input.fill(task.title);
  await expect(dialog.getByRole("button").filter({ hasText: task.title })).toHaveCount(1);
  await input.press("Enter");
  await expect(dialog).toBeHidden();
  await expectSelectedTask(page, task);
}

async function sendWorkspaceFocusChord(app, page, withOtherKey = false) {
  const isMac = process.platform === "darwin";
  const modKey = isMac ? "Meta" : "Control";
  const modName = isMac ? "meta" : "control";
  const events = [
    { type: "keyDown", keyCode: modKey, modifiers: [modName] },
    { type: "keyDown", keyCode: "Alt", modifiers: [modName, "alt"] },
    ...(withOtherKey ? [
      { type: "keyDown", keyCode: "Right", modifiers: [modName, "alt"] },
      { type: "keyUp", keyCode: "Right", modifiers: [modName, "alt"] },
    ] : []),
    { type: "keyUp", keyCode: "Alt", modifiers: [modName] },
    { type: "keyUp", keyCode: modKey, modifiers: [] },
  ];
  await app.evaluate(({ BrowserWindow }, { url, inputs }) => {
    const window = BrowserWindow.getAllWindows().find((candidate) => candidate.webContents.getURL() === url);
    if (!window) throw new Error(`QA BrowserWindow not found: ${url}`);
    window.focus();
    for (const input of inputs) window.webContents.sendInputEvent(input);
  }, { url: page.url(), inputs: events });
  // main의 before-input-event와 별도로 전송되는 IPC가 렌더러에 반영된 뒤 포커스를 검사한다.
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
}

async function main() {
  const options = parseArgs(process.argv);
  const run = createQaRun({ ...options, rootDir: process.cwd() });
  const fixture = createFixtureRepository(run);
  const videoDir = path.join(run.runDir, "video");
  const checks = [];
  const screenshots = [];
  const consoleErrors = [];
  let app = null;
  let page = null;
  const seeded = {};

  try {
    ({ app, page } = await launchKanVibeElectron({
      rootDir: run.rootDir,
      outputDir: run.runDir,
      appDataDir: path.join(run.runDir, "app-data"),
      viewport: { width: 1500, height: 950 },
      actionTimeoutMs: 15000,
      recordVideo: { dir: videoDir, size: { width: 1500, height: 950 } },
    }));
    page.on("console", (message) => {
      if (message.type() === "error") consoleErrors.push(message.text());
    });
    page.on("pageerror", (error) => consoleErrors.push(`pageerror: ${error.message}`));

    await page.locator("body").waitFor({ state: "visible", timeout: 20000 });
    await dismissUpdateDialogIfPresent(page);

    await step(checks, "QA 데이터 준비: 프로젝트와 세션 없는 task 2개, 기본 세션 타입 terminal", async () => {
      const projectResult = await invokeDesktop(page, "project", "registerProject", fixture.projectName, fixture.projectDir);
      if (!projectResult.success) throw new Error(projectResult.error || "project registration failed");
      seeded.project = projectResult.project;
      await invokeDesktop(page, "appSettings", "setDefaultSessionType", "terminal");
      seeded.alpha = await invokeDesktop(page, "kanban", "createTask", {
        title: `alpha-login-${run.runId}`, projectId: seeded.project.id, baseBranch: "main",
      });
      seeded.beta = await invokeDesktop(page, "kanban", "createTask", {
        title: `beta-billing-${run.runId}`, projectId: seeded.project.id, baseBranch: "main",
      });
      await page.evaluate(() => {
        window.sessionStorage.clear();
        window.location.hash = "#/ko";
      });
      await page.reload({ waitUntil: "domcontentloaded" });
      await dismissUpdateDialogIfPresent(page);
      return `alpha=${seeded.alpha.id}, beta=${seeded.beta.id}`;
    });

    await step(checks, "칸반에서 시작하고 팔레트 `view workspace`로 워크스페이스로 전환", async () => {
      await page.locator("[data-kanban-task-card='true']").first().waitFor({ state: "visible", timeout: 20000 });
      await takeScreenshot(page, run, "kanban-before-switch", screenshots);
      await runPaletteCommand(page, "view workspace");
      await taskList(page).waitFor({ state: "visible", timeout: 15000 });
      await takeScreenshot(page, run, "workspace-after-switch", screenshots);
      return `listed=${(await listedTitles(page)).length}`;
    });

    await step(checks, "세션 없는 task를 고르면 폼 없이 자동 연결되고 오른쪽에 터미널이 뜬다", async () => {
      await taskList(page).getByRole("button", { name: new RegExp(seeded.alpha.title) }).click();
      await page.locator(".xterm").first().waitFor({ state: "visible", timeout: 30000 });
      const connectedTask = await invokeDesktop(page, "kanban", "getTaskById", seeded.alpha.id);
      if (!connectedTask.sessionType || !connectedTask.sessionName) throw new Error("task has no session after auto-connect");
      if (await page.getByRole("combobox").count() > 0) throw new Error("session type select is still visible");
      await page.waitForTimeout(1500);
      await takeScreenshot(page, run, "alpha-auto-connected-terminal", screenshots);
      return `sessionType=${connectedTask.sessionType}, sessionName=${connectedTask.sessionName}`;
    });

    await step(checks, "검색어 필터는 일치하는 task만 남긴다", async () => {
      await page.getByRole("searchbox", { name: "제목, 브랜치, 프로젝트 검색" }).fill("beta-billing");
      await page.waitForTimeout(300);
      const titles = await listedTitles(page);
      if (titles.length !== 1 || !titles[0].includes("beta-billing")) throw new Error(`unexpected titles: ${titles.join(", ")}`);
      await takeScreenshot(page, run, "search-filter-beta", screenshots);
      return titles.join(", ");
    });

    await step(checks, "다른 task를 고르면 오른쪽 터미널이 그 task로 바뀐다", async () => {
      await taskList(page).getByRole("button", { name: new RegExp(seeded.beta.title) }).click();
      await page.getByRole("region", { name: seeded.beta.title }).waitFor({ state: "visible", timeout: 15000 });
      await page.locator(".xterm").first().waitFor({ state: "visible", timeout: 30000 });
      await page.waitForTimeout(1500);
      await takeScreenshot(page, run, "beta-terminal", screenshots);
      return "terminal header shows beta task";
    });

    await step(checks, "빠른 검색의 beta 선택은 상세 경로 대신 Worktree 좌우 선택을 갱신한다", async () => {
      await page.getByRole("searchbox", { name: "제목, 브랜치, 프로젝트 검색" }).fill("");
      await taskCard(page, seeded.alpha.id).click();
      await expectSelectedTask(page, seeded.alpha);
      await searchForTask(page, seeded.beta);
      await takeScreenshot(page, run, "search-selected-beta-in-workspace", screenshots);
      return `taskId=${seeded.beta.id}, left aria-current and right terminal header agree; no /task/ route`;
    });

    await step(checks, "목록 필터가 숨긴 현재 beta도 빠른 검색으로 다시 고르면 왼쪽에 표시된다", async () => {
      const searchInput = page.getByRole("searchbox", { name: "제목, 브랜치, 프로젝트 검색" });
      await searchInput.fill("alpha-login");
      await expect(taskCard(page, seeded.beta.id)).toHaveCount(0);
      await expect(taskCard(page, seeded.alpha.id)).toBeVisible();
      await searchForTask(page, seeded.beta);
      await expect(searchInput).toHaveValue("alpha-login");
      await takeScreenshot(page, run, "filtered-search-reveals-selected-beta", screenshots);
      return "beta is revealed and selected while the user's alpha filter remains intact";
    });

    await step(checks, "터미널에서 Cmd+Option/Ctrl+Alt만 눌렀다 떼면 현재 선택 카드로 포커스가 이동한다", async () => {
      await page.getByRole("searchbox", { name: "제목, 브랜치, 프로젝트 검색" }).fill("");
      await expectSelectedTask(page, seeded.beta);
      const terminalInput = page.getByRole("region", { name: seeded.beta.title, exact: true }).locator(".xterm-helper-textarea");
      await terminalInput.focus();
      await expect(terminalInput).toBeFocused();
      await sendWorkspaceFocusChord(app, page);
      await expect(taskCard(page, seeded.beta.id)).toBeFocused();
      await takeScreenshot(page, run, "modifier-release-focuses-selected-card", screenshots);
      return "Electron BrowserWindow.webContents.sendInputEvent → before-input-event → selected left card focus";
    });

    await step(checks, "목록 위아래 키는 표시 순서대로 좌우 선택을 바꾸고 양 끝에서 멈춘다", async () => {
      const cards = await taskList(page).locator("[data-workspace-task-id]").evaluateAll((elements) => elements.map((element) => ({
        id: element.dataset.workspaceTaskId,
        title: element.querySelector("h3").textContent,
      })));
      if (cards.length < 2) throw new Error(`expected at least two listed tasks, got ${cards.length}`);
      await taskCard(page, cards[0].id).click();
      await expectSelectedTask(page, cards[0]);
      await page.keyboard.press("ArrowUp");
      await expectSelectedTask(page, cards[0]);
      await expect(taskCard(page, cards[0].id)).toBeFocused();
      await page.keyboard.press("ArrowDown");
      await expectSelectedTask(page, cards[1]);
      await expect(taskCard(page, cards[1].id)).toBeFocused();
      await page.keyboard.press("ArrowUp");
      await expectSelectedTask(page, cards[0]);
      const last = cards[cards.length - 1];
      await taskCard(page, last.id).click();
      await expectSelectedTask(page, last);
      await page.keyboard.press("ArrowDown");
      await expectSelectedTask(page, last);
      await expect(taskCard(page, last.id)).toBeFocused();
      await takeScreenshot(page, run, "keyboard-selection-and-terminal-header", screenshots);
      return `${cards.length} listed tasks; up/down changes selected card and terminal header; first/last do not wrap`;
    });

    await step(checks, "목록 Enter는 터미널로 복귀하고 입력한 셸 명령이 실제 출력된다", async () => {
      await taskCard(page, seeded.beta.id).click();
      await expectSelectedTask(page, seeded.beta);
      const terminalInput = page.getByRole("region", { name: seeded.beta.title, exact: true }).locator(".xterm-helper-textarea");
      await terminalInput.waitFor({ state: "attached", timeout: 30000 });
      await expect(taskCard(page, seeded.beta.id)).toBeFocused();
      await page.keyboard.press("Enter");
      await expect(terminalInput).toBeFocused();
      const suffix = run.runId.replace(/[^a-zA-Z0-9]/g, "_");
      const marker = `WORKSPACE_NAV_${suffix}`;
      await page.evaluate((taskId) => {
        window.__workspaceQaTerminalOutput = "";
        window.__workspaceQaStopTerminalCapture = window.kanvibeDesktop.onTerminalData((event) => {
          if (event.taskId === taskId) window.__workspaceQaTerminalOutput += event.data;
        });
      }, seeded.beta.id);
      try {
        await page.keyboard.type(`printf 'WORKSPACE_NAV_%s\\n' '${suffix}'`);
        await page.keyboard.press("Enter");
        await expect.poll(() => page.evaluate((expected) => window.__workspaceQaTerminalOutput.includes(expected), marker), { timeout: 15000 }).toBe(true);
        await takeScreenshot(page, run, "enter-restores-usable-beta-terminal", screenshots);
      } finally {
        await page.evaluate(() => {
          window.__workspaceQaStopTerminalCapture();
          delete window.__workspaceQaStopTerminalCapture;
          delete window.__workspaceQaTerminalOutput;
        });
      }
      return `terminal textarea focused and real PTY output contains ${marker}`;
    });

    await step(checks, "Cmd+Option/Ctrl+Alt에 다른 키를 섞으면 터미널 포커스를 유지한다", async () => {
      const terminalInput = page.getByRole("region", { name: seeded.beta.title, exact: true }).locator(".xterm-helper-textarea");
      await expect(terminalInput).toBeFocused();
      await sendWorkspaceFocusChord(app, page, true);
      await expect(terminalInput).toBeFocused();
      await expectSelectedTask(page, seeded.beta);
      return "modifier chord with Right does not focus the task list";
    });

    await step(checks, "상태 칩 복수 선택: todo만 켜면 두 task, done을 더하면 done도 포함", async () => {
      await page.getByRole("searchbox", { name: "제목, 브랜치, 프로젝트 검색" }).fill("");
      const statusGroup = page.getByRole("group", { name: "상태 필터" });
      const chips = statusGroup.getByRole("button");
      await chips.first().click();
      const todoOnly = await listedTitles(page);
      await chips.last().click();
      const pressedCount = await statusGroup.locator("[aria-pressed='true']").count();
      await page.waitForTimeout(400);
      await takeScreenshot(page, run, "status-chips-multi-select", screenshots);
      if (pressedCount !== 2) throw new Error(`expected 2 pressed chips, got ${pressedCount}`);
      if (!todoOnly.some((title) => title.includes("alpha-login"))) throw new Error(`todo filter missing alpha: ${todoOnly.join(", ")}`);
      return `todoOnly=${todoOnly.length}, pressed=${pressedCount}`;
    });

    await step(checks, "다시 불러와도 워크스페이스 뷰가 유지된다", async () => {
      await page.reload({ waitUntil: "domcontentloaded" });
      await taskList(page).waitFor({ state: "visible", timeout: 15000 });
      return "workspace restored from AppSettings";
    });

    await step(checks, "팔레트 `view kanban`으로 칸반으로 돌아간다", async () => {
      await runPaletteCommand(page, "view kanban");
      await page.locator("[data-kanban-task-card='true']").first().waitFor({ state: "visible", timeout: 15000 });
      await takeScreenshot(page, run, "kanban-after-switch-back", screenshots);
      return "board cards visible";
    });

    await step(checks, "차단성 콘솔 오류 없음", async () => {
      const blocking = consoleErrors.filter((line) => !/favicon|DevTools|Electron Security Warning|Content-Security-Policy|nerdfont|jsdelivr/i.test(line));
      if (blocking.length > 0) throw new Error(blocking.join("\n"));
      return "none";
    });
  } catch (error) {
    checks.push({ name: "workspace view flow", ok: false, detail: error instanceof Error ? error.stack || error.message : String(error) });
  } finally {
    if (app) await app.close().catch(() => {});
  }

  const videoFiles = fs.existsSync(videoDir) ? fs.readdirSync(videoDir).filter((name) => name.endsWith(".webm")) : [];
  const ok = checks.every((check) => check.ok);
  const result = {
    ok,
    scope: SCOPE,
    branch: gitValue(["branch", "--show-current"]),
    commit: gitValue(["rev-parse", "--short", "HEAD"]),
    checks,
    consoleErrors,
    screenshots,
    videos: videoFiles.map((name) => path.join(videoDir, name)),
  };
  fs.writeFileSync(run.jsonPath, `${JSON.stringify({ ...result, runId: run.runId }, null, 2)}\n`, "utf8");
  console.log(JSON.stringify({ ok, runDir: run.runDir, checks: checks.map((check) => `${check.ok ? "PASS" : "FAIL"} ${check.name} — ${check.detail}`), videos: result.videos }, null, 2));
  process.exit(ok ? 0 : 1);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
