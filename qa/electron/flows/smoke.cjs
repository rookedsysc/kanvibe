#!/usr/bin/env node
/* eslint-disable @typescript-eslint/no-require-imports */
const fs = require("node:fs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const { createQaRun, writeReport } = require("../lib/report.cjs");
const { createFixtureRepository } = require("../lib/fixtureRepository.cjs");
const { launchKanVibeElectron } = require("../lib/launchElectron.cjs");

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

function execGit(args, cwd) {
  return execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

function getGitWorktrees(projectDir) {
  try {
    return execGit(["worktree", "list", "--porcelain"], projectDir);
  } catch (error) {
    return error instanceof Error ? error.message : String(error);
  }
}

function branchExists(projectDir, branchName) {
  try {
    execGit(["show-ref", "--verify", "--quiet", `refs/heads/${branchName}`], projectDir);
    return true;
  } catch {
    return false;
  }
}

function writeJson(filePath, data) {
  fs.writeFileSync(filePath, `${JSON.stringify(data, null, 2)}\n`, "utf8");
}

async function takeScreenshot(page, run, label, screenshots) {
  const fileName = `${String(screenshots.length + 1).padStart(2, "0")}-${label}.png`;
  const shotPath = path.join(run.screenshotsDir, fileName);
  await page.screenshot({ path: shotPath, fullPage: true });
  screenshots.push({ label, path: shotPath });
}

function createCdpDiagnosticsCollector() {
  const events = [];
  const counters = {
    requests: 0,
    responses: 0,
    failedRequests: 0,
    exceptions: 0,
    logEntries: 0,
  };

  const pushEvent = (type, payload) => {
    if (events.length >= 200) return;
    events.push({ type, timestamp: new Date().toISOString(), payload });
  };

  return {
    events,
    counters,
    pushEvent,
    summary() {
      return { counters, sampledEvents: events };
    },
  };
}

async function attachCdpDiagnostics(page, collector) {
  const cdpSession = await page.context().newCDPSession(page);
  await Promise.all([
    cdpSession.send("Runtime.enable"),
    cdpSession.send("Log.enable"),
    cdpSession.send("Network.enable"),
    cdpSession.send("Performance.enable"),
  ]);

  cdpSession.on("Runtime.exceptionThrown", (event) => {
    collector.counters.exceptions += 1;
    collector.pushEvent("Runtime.exceptionThrown", {
      text: event.exceptionDetails?.text,
      url: event.exceptionDetails?.url,
      lineNumber: event.exceptionDetails?.lineNumber,
    });
  });
  cdpSession.on("Log.entryAdded", (event) => {
    collector.counters.logEntries += 1;
    collector.pushEvent("Log.entryAdded", {
      level: event.entry?.level,
      text: event.entry?.text,
      url: event.entry?.url,
    });
  });
  cdpSession.on("Network.requestWillBeSent", () => {
    collector.counters.requests += 1;
  });
  cdpSession.on("Network.responseReceived", () => {
    collector.counters.responses += 1;
  });
  cdpSession.on("Network.loadingFailed", (event) => {
    collector.counters.failedRequests += 1;
    collector.pushEvent("Network.loadingFailed", {
      requestId: event.requestId,
      errorText: event.errorText,
      canceled: event.canceled,
    });
  });

  return cdpSession;
}

async function optionalStep(checks, name, fn) {
  try {
    const detail = await fn();
    checks.push({ name, ok: true, detail: detail || "ok" });
    return detail;
  } catch (error) {
    checks.push({ name, ok: false, detail: error instanceof Error ? error.message : String(error) });
    return null;
  }
}

async function invokeDesktop(page, namespace, method, ...args) {
  return page.evaluate(
    async ({ namespace, method, args }) => window.kanvibeDesktop.invoke(namespace, method, args),
    { namespace, method, args },
  );
}

async function dismissUpdateDialogIfPresent(page) {
  const closeButton = page.getByRole("button", { name: "닫기" }).first();
  try {
    await closeButton.click({ timeout: 2500 });
    await page.waitForTimeout(600);
  } catch {
    // The update/release-note dialog is optional and may already be suppressed.
  }
}

async function seedQaProjectAndTask(page, fixture) {
  return page.evaluate(async ({ projectName, projectDir, branchName, baseBranch }) => {
    const projectResult = await window.kanvibeDesktop.invoke("project", "registerProject", [projectName, projectDir]);
    if (!projectResult.success || !projectResult.project) {
      throw new Error(projectResult.error || "QA project registration failed");
    }

    const task = await window.kanvibeDesktop.invoke("kanban", "createTask", [{
      title: branchName,
      description: "Electron QA: UI status change and delete must clean a real git worktree located outside KanVibe's managed __worktrees convention, even when DB worktreePath is empty.",
      branchName,
      baseBranch,
      projectId: projectResult.project.id,
    }]);

    window.localStorage.clear();
    window.sessionStorage.clear();
    return { project: projectResult.project, task };
  }, fixture);
}

async function waitForTaskCard(page, taskId) {
  const selector = `[data-kanban-task-card='true'][data-kanban-task-id='${taskId}']`;
  const card = page.locator(selector);
  await card.waitFor({ state: "visible", timeout: 20000 });
  await card.scrollIntoViewIfNeeded();
  return card;
}

async function changeTaskStatusFromContextMenu(page, taskId, statusIndex) {
  const card = await waitForTaskCard(page, taskId);
  await card.click({ button: "right" });
  await page.waitForTimeout(700);
  await page.locator("[role='menuitem'][aria-haspopup='menu']").click();
  await page.waitForTimeout(700);
  await page.locator("[role='menuitemradio']").nth(statusIndex).click();
  await page.waitForTimeout(1200);
}

async function deleteTaskFromContextMenu(page, taskId) {
  const card = await waitForTaskCard(page, taskId);
  page.once("dialog", async (dialog) => {
    await dialog.accept();
  });
  await card.click({ button: "right" });
  await page.waitForTimeout(700);
  await page.locator("[role='menu'] > button").last().click();
  await page.waitForTimeout(1800);
}

function taskCardSelector(taskId) {
  return `[data-kanban-task-card='true'][data-kanban-task-id='${taskId}']`;
}

async function createPlainQaTask(page, projectId, title) {
  return invokeDesktop(page, "kanban", "createTask", {
    title,
    description: "Electron QA: Vim-style board shortcut coverage task.",
    projectId,
  });
}

async function moveTaskStatusThroughBackend(page, taskId, status) {
  await invokeDesktop(page, "kanban", "moveTaskToColumn", taskId, status, [taskId]);
}

async function seedVimShortcutTasks(page, projectId, runId) {
  const navTodoTop = await createPlainQaTask(page, projectId, `vim-qa-${runId}-todo-top`);
  const navTodoBottom = await createPlainQaTask(page, projectId, `vim-qa-${runId}-todo-bottom`);
  const navProgressPeer = await createPlainQaTask(page, projectId, `vim-qa-${runId}-progress-peer`);
  const commandTask = await createPlainQaTask(page, projectId, `vim-qa-${runId}-command-cycle`);
  const deleteTask = await createPlainQaTask(page, projectId, `vim-qa-${runId}-dd-delete`);

  await moveTaskStatusThroughBackend(page, navProgressPeer.id, "progress");

  return { navTodoTop, navTodoBottom, navProgressPeer, commandTask, deleteTask };
}

async function navigateToBoard(page) {
  await page.evaluate(() => {
    window.location.hash = "#/ko";
  });
  await page.waitForTimeout(800);
  await dismissUpdateDialogIfPresent(page);
}

async function openSettingsFromBoard(page) {
  await navigateToBoard(page);
  await page.getByRole("button", { name: /설정|Settings/ }).click({ timeout: 10000 });
  const vimSwitch = page.getByRole("switch", { name: /Vim/i });
  await vimSwitch.waitFor({ state: "visible", timeout: 10000 });
  await page.waitForTimeout(600);
  return vimSwitch;
}

async function setVimModeFromSettings(page, enabled) {
  const vimSwitch = await openSettingsFromBoard(page);
  const current = (await vimSwitch.getAttribute("aria-checked")) === "true";
  if (current !== enabled) {
    await vimSwitch.click();
    await page.waitForTimeout(800);
  }

  await page.waitForFunction(
    async (expected) => window.kanvibeDesktop.invoke("appSettings", "getVimModeEnabled", []).then((value) => value === expected),
    enabled,
    { timeout: 10000 },
  );
  await navigateToBoard(page);
}

async function blurActiveElement(page) {
  await page.evaluate(() => {
    if (document.activeElement instanceof HTMLElement) {
      document.activeElement.blur();
    }
  });
}

async function focusTaskCard(page, taskId) {
  const card = await waitForTaskCard(page, taskId);
  await card.focus();
  await page.waitForTimeout(500);
}

async function getFocusedTaskInfo(page) {
  return page.evaluate(() => {
    const activeElement = document.activeElement;
    const card = activeElement instanceof Element
      ? activeElement.closest("[data-kanban-task-card='true']")
      : null;
    return {
      taskId: card?.getAttribute("data-kanban-task-id") ?? null,
      status: card?.getAttribute("data-kanban-status") ?? null,
    };
  });
}

async function expectFocusedTask(page, taskId, context) {
  await page.waitForFunction(
    (expectedTaskId) => {
      const activeElement = document.activeElement;
      const card = activeElement instanceof Element
        ? activeElement.closest("[data-kanban-task-card='true']")
        : null;
      return card?.getAttribute("data-kanban-task-id") === expectedTaskId;
    },
    taskId,
    { timeout: 7000 },
  ).catch(async (error) => {
    const focused = await getFocusedTaskInfo(page);
    throw new Error(`${context}: expected focused task ${taskId}, got ${JSON.stringify(focused)} (${error.message})`);
  });
}

async function expectFocusedStatus(page, status, context) {
  const focused = await getFocusedTaskInfo(page);
  if (focused.status !== status) {
    throw new Error(`${context}: expected focused status ${status}, got ${JSON.stringify(focused)}`);
  }
  return focused;
}

async function waitForTaskStatus(page, taskId, status) {
  await page.locator(`${taskCardSelector(taskId)}[data-kanban-status='${status}']`).waitFor({ state: "visible", timeout: 15000 });
  const task = await invokeDesktop(page, "kanban", "getTaskById", taskId);
  if (task?.status !== status) {
    throw new Error(`expected backend status ${status} for ${taskId}, got ${task?.status}`);
  }
  return task;
}

async function openCreateTaskModalWithVimN(page) {
  await blurActiveElement(page);
  await page.keyboard.press("n");
  await page.locator("input[name='branchName']").waitFor({ state: "visible", timeout: 7000 });
  await page.waitForTimeout(700);
}

async function assertCreateTaskModalClosed(page, context) {
  await page.waitForTimeout(800);
  const isVisible = await page.locator("input[name='branchName']").first().isVisible().catch(() => false);
  if (isVisible) {
    await page.keyboard.press("Escape").catch(() => {});
    throw new Error(`${context}: create task modal unexpectedly opened`);
  }
}

async function pressVimCommandShortcut(page) {
  // Playwright emits key=';' for Shift+; on Linux, while the app's shortcut
  // handler listens for the actual ':' key value produced by users typing ':'.
  await page.keyboard.press(":");
}

async function submitVimMoveCommand(page, taskId, status) {
  await focusTaskCard(page, taskId);
  await pressVimCommandShortcut(page);
  const commandInput = page.locator("#vim-command-input");
  await commandInput.waitFor({ state: "visible", timeout: 7000 });
  await page.keyboard.type(`move ${status}`, { delay: 25 });
  await page.waitForTimeout(250);
  await page.keyboard.press("Enter");
  await page.waitForTimeout(900);
  await waitForTaskStatus(page, taskId, status);
}

async function assertVimCommandClosed(page, context) {
  await page.waitForTimeout(700);
  const isVisible = await page.locator("#vim-command-input").first().isVisible().catch(() => false);
  if (isVisible) {
    await page.keyboard.press("Escape").catch(() => {});
    throw new Error(`${context}: Vim command input unexpectedly opened`);
  }
}

async function captureDialogs(page, fn, action = "dismiss") {
  const dialogs = [];
  const handler = async (dialog) => {
    dialogs.push({ type: dialog.type(), message: dialog.message() });
    if (action === "accept") {
      await dialog.accept();
    } else {
      await dialog.dismiss();
    }
  };

  page.on("dialog", handler);
  try {
    await fn();
  } finally {
    page.off("dialog", handler);
  }
  return dialogs;
}

async function deleteTaskWithVimDd(page, taskId) {
  let lastFocused = null;
  for (let attempt = 1; attempt <= 8; attempt += 1) {
    await focusTaskCard(page, taskId);
    await expectFocusedTask(page, taskId, `Vim dd delete attempt ${attempt} should focus target task`);
    await page.waitForTimeout(500);
    lastFocused = await getFocusedTaskInfo(page);

    const dialogPromise = page.waitForEvent("dialog", { timeout: 1800 })
      .then(async (dialog) => {
        const info = { type: dialog.type(), message: dialog.message(), attempt };
        await dialog.accept();
        return info;
      })
      .catch(() => null);

    await page.keyboard.press("d");
    await page.waitForTimeout(700);
    await page.keyboard.press("d");

    const dialog = await dialogPromise;
    if (dialog) {
      return dialog;
    }

    await page.waitForTimeout(500);
  }

  const task = await invokeDesktop(page, "kanban", "getTaskById", taskId);
  throw new Error(`expected one delete confirmation dialog, got none after 8 attempts; focused=${JSON.stringify(lastFocused)}, task=${JSON.stringify(task)}`);
}

async function assertTaskExists(page, taskId, context) {
  const task = await invokeDesktop(page, "kanban", "getTaskById", taskId);
  if (!task) {
    throw new Error(`${context}: expected task ${taskId} to still exist`);
  }
  return task;
}

async function assertTaskDeleted(page, taskId, context) {
  await page.locator(taskCardSelector(taskId)).waitFor({ state: "hidden", timeout: 15000 }).catch(async (error) => {
    const focused = await getFocusedTaskInfo(page);
    throw new Error(`${context}: task card ${taskId} is still visible; focused=${JSON.stringify(focused)} (${error.message})`);
  });

  await page.waitForFunction(
    async (id) => {
      const task = await window.kanvibeDesktop.invoke("kanban", "getTaskById", [id]);
      return task === null;
    },
    taskId,
    { timeout: 15000 },
  ).catch(async (error) => {
    const task = await invokeDesktop(page, "kanban", "getTaskById", taskId);
    throw new Error(`${context}: task still exists (${JSON.stringify(task)}) (${error.message})`);
  });
}

async function main() {
  const args = parseArgs(process.argv);
  const run = createQaRun({
    rootDir: process.cwd(),
    outputRoot: args.outputRoot,
    runId: args.runId,
    runDir: args.runDir,
  });
  const fixture = createFixtureRepository(run);

  const consoleErrors = [];
  const diagnostics = [];
  const cdpCollector = createCdpDiagnosticsCollector();
  const checks = [];
  const screenshots = [];
  const notes = [];
  let app;
  let page;
  let cdpSession;
  let traceStarted = false;
  let ok = false;
  let seeded;

  try {
    const launched = await launchKanVibeElectron({
      rootDir: process.cwd(),
      outputDir: run.runDir,
      appDataDir: path.join(run.runDir, "app-data"),
    });
    app = launched.app;
    page = launched.page;

    await page.context().tracing.start({ screenshots: true, snapshots: true, sources: true });
    traceStarted = true;

    await optionalStep(checks, "CDP diagnostics session attaches to Electron renderer", async () => {
      cdpSession = await attachCdpDiagnostics(page, cdpCollector);
      return `remote-debugging-port=${launched.cdpPort}`;
    });

    page.on("console", (message) => {
      if (["error", "warning"].includes(message.type())) {
        consoleErrors.push(`${message.type()}: ${message.text()}`);
      }
    });
    page.on("pageerror", (error) => consoleErrors.push(`pageerror: ${error.message}`));
    page.on("requestfailed", (request) => {
      const failure = request.failure();
      consoleErrors.push(`requestfailed: ${request.url()} ${failure?.errorText || "unknown"}`);
    });

    await optionalStep(checks, "Electron window opens", async () => {
      await page.waitForLoadState("domcontentloaded");
      await dismissUpdateDialogIfPresent(page);
      return page.url();
    });

    await optionalStep(checks, "Renderer becomes visible", async () => {
      await page.locator("body").waitFor({ state: "visible", timeout: 20000 });
      return "body visible";
    });

    await optionalStep(checks, "QA fixture contains real non-managed external git worktree before UI delete", async () => {
      const worktrees = getGitWorktrees(fixture.projectDir);
      if (fixture.worktreePath.includes("__worktrees") || fixture.worktreePath.startsWith(path.dirname(fixture.projectDir))) {
        throw new Error(`fixture worktree is not external/non-managed: ${fixture.worktreePath}`);
      }
      if (!worktrees.includes(fixture.worktreePath) || !branchExists(fixture.projectDir, fixture.branchName)) {
        throw new Error(`fixture branch/worktree missing before QA\n${worktrees}`);
      }
      return `${fixture.branchName} at ${fixture.worktreePath}`;
    });

    let seededResult = null;
    let vimTasks = null;
    await optionalStep(checks, "Seed isolated QA project, cleanup task, and Vim shortcut tasks through app IPC", async () => {
      seededResult = await seedQaProjectAndTask(page, fixture);
      await invokeDesktop(page, "appSettings", "setVimModeEnabled", true);
      vimTasks = await seedVimShortcutTasks(page, seededResult.project.id, run.runId);
      await page.reload({ waitUntil: "domcontentloaded" });
      await dismissUpdateDialogIfPresent(page);
      return `project=${seededResult.project.name}, cleanupTask=${seededResult.task.id}, vimTasks=${Object.values(vimTasks).map((task) => task.id).join(",")}`;
    });
    seeded = seededResult;

    if (!seeded || !vimTasks) {
      throw new Error("QA seed failed; cannot continue UI flow");
    }

    await optionalStep(checks, "Board shows seeded cleanup and Vim QA task cards", async () => {
      await waitForTaskCard(page, seeded.task.id);
      await waitForTaskCard(page, vimTasks.navTodoTop.id);
      await waitForTaskCard(page, vimTasks.navTodoBottom.id);
      await waitForTaskStatus(page, vimTasks.navProgressPeer.id, "progress");
      return `${fixture.branchName}; ${Object.values(vimTasks).map((task) => task.title).join(", ")}`;
    });

    await page.waitForTimeout(1500);
    await takeScreenshot(page, run, "seeded-vim-task-cards-visible", screenshots);

    await optionalStep(checks, "UI context menu changes cleanup task status TODO to PROGRESS", async () => {
      await changeTaskStatusFromContextMenu(page, seeded.task.id, 1);
      await waitForTaskStatus(page, seeded.task.id, "progress");
      return "card moved to PROGRESS and backend status updated";
    });

    await takeScreenshot(page, run, "cleanup-task-moved-to-progress", screenshots);
    await page.waitForTimeout(1200);

    await optionalStep(checks, "Vim h/j/k/l moves focus across task cards when setting is ON", async () => {
      await focusTaskCard(page, vimTasks.navTodoTop.id);
      await page.keyboard.press("j");
      await expectFocusedTask(page, vimTasks.navTodoBottom.id, "j should move to next TODO task");
      await page.waitForTimeout(500);
      await page.keyboard.press("k");
      await expectFocusedTask(page, vimTasks.navTodoTop.id, "k should move back to previous TODO task");
      await page.waitForTimeout(500);
      await page.keyboard.press("l");
      const rightFocus = await expectFocusedStatus(page, "progress", "l should move to the next non-empty status column");
      await page.waitForTimeout(500);
      await page.keyboard.press("h");
      await expectFocusedTask(page, vimTasks.navTodoTop.id, "h should move back to TODO at the nearest index");
      return `j/k reached ${vimTasks.navTodoBottom.id}/${vimTasks.navTodoTop.id}; l reached ${rightFocus.taskId} in PROGRESS; h returned to TODO`;
    });

    await takeScreenshot(page, run, "vim-hjkl-focus-navigation-on", screenshots);

    await optionalStep(checks, "Vim :move command moves focused task through every supported status", async () => {
      const statuses = ["progress", "pending", "review", "done", "todo"];
      for (const status of statuses) {
        await submitVimMoveCommand(page, vimTasks.commandTask.id, status);
      }
      await focusTaskCard(page, vimTasks.commandTask.id);
      return `:move ${statuses.join(" -> :move ")} all updated UI and backend state`;
    });

    await takeScreenshot(page, run, "vim-command-move-all-statuses", screenshots);

    await optionalStep(checks, "Vim n opens the create-task modal when setting is ON", async () => {
      await openCreateTaskModalWithVimN(page);
      await page.keyboard.press("Escape");
      await assertCreateTaskModalClosed(page, "Escape should close Vim-created task modal");
      return "n opened Create Task modal; Escape closed it";
    });

    await optionalStep(checks, "Vim dd deletes the focused task after confirmation when setting is ON", async () => {
      const dialog = await deleteTaskWithVimDd(page, vimTasks.deleteTask.id);
      if (!/삭제|delete/i.test(dialog.message)) {
        throw new Error(`expected delete confirmation message, got ${JSON.stringify(dialog)}`);
      }
      await assertTaskDeleted(page, vimTasks.deleteTask.id, "Vim dd delete");
      return `confirmed dd delete dialog on attempt ${dialog.attempt} and removed ${vimTasks.deleteTask.id}`;
    });

    await takeScreenshot(page, run, "vim-dd-delete-complete", screenshots);

    await optionalStep(checks, "Settings toggle OFF disables Vim-only shortcuts but keeps arrow-key focus navigation", async () => {
      await setVimModeFromSettings(page, false);
      await takeScreenshot(page, run, "vim-mode-toggle-off", screenshots);

      await blurActiveElement(page);
      await page.keyboard.press("n");
      await assertCreateTaskModalClosed(page, "Vim OFF n shortcut");

      await focusTaskCard(page, vimTasks.navTodoTop.id);
      await page.keyboard.press("l");
      await expectFocusedTask(page, vimTasks.navTodoTop.id, "Vim OFF l shortcut should not move focus");
      await page.keyboard.press("ArrowRight");
      const arrowFocus = await expectFocusedStatus(page, "progress", "ArrowRight should still move focus while Vim mode is OFF");

      await focusTaskCard(page, vimTasks.commandTask.id);
      await pressVimCommandShortcut(page);
      await assertVimCommandClosed(page, "Vim OFF : command shortcut");

      const dialogs = await captureDialogs(page, async () => {
        await focusTaskCard(page, vimTasks.commandTask.id);
        await page.keyboard.press("d");
        await page.waitForTimeout(180);
        await page.keyboard.press("d");
        await page.waitForTimeout(900);
      }, "dismiss");
      if (dialogs.length > 0) {
        throw new Error(`Vim OFF dd unexpectedly opened dialog(s): ${JSON.stringify(dialogs)}`);
      }
      await assertTaskExists(page, vimTasks.commandTask.id, "Vim OFF dd shortcut");

      return `n/:/dd/l disabled; ArrowRight still moved focus to ${arrowFocus.taskId}`;
    });

    await optionalStep(checks, "Settings toggle ON restores Vim shortcut handling", async () => {
      await setVimModeFromSettings(page, true);
      await takeScreenshot(page, run, "vim-mode-toggle-on", screenshots);
      await openCreateTaskModalWithVimN(page);
      await page.keyboard.press("Escape");
      await assertCreateTaskModalClosed(page, "Vim ON n shortcut after re-enable");
      await focusTaskCard(page, vimTasks.commandTask.id);
      await pressVimCommandShortcut(page);
      await page.locator("#vim-command-input").waitFor({ state: "visible", timeout: 7000 });
      await page.keyboard.press("Escape");
      await assertVimCommandClosed(page, "Vim command input after re-enable Escape");
      return "Vim n and : shortcuts worked again after re-enable";
    });

    await takeScreenshot(page, run, "vim-shortcuts-restored", screenshots);

    await optionalStep(checks, "UI delete removes cleanup task record", async () => {
      await deleteTaskFromContextMenu(page, seeded.task.id);
      await assertTaskDeleted(page, seeded.task.id, "context-menu cleanup task delete");
      return "backend getTaskById returned null after confirm delete";
    });

    await page.reload({ waitUntil: "domcontentloaded" });
    await page.waitForTimeout(1500);
    await takeScreenshot(page, run, "cleanup-task-deleted-after-refresh", screenshots);

    await optionalStep(checks, "Delete cleanup removes actual external git worktree and branch", async () => {
      const worktrees = getGitWorktrees(fixture.projectDir);
      if (worktrees.includes(fixture.worktreePath)) {
        throw new Error(`worktree still present after delete\n${worktrees}`);
      }
      if (branchExists(fixture.projectDir, fixture.branchName)) {
        throw new Error(`branch still present after delete: ${fixture.branchName}`);
      }
      if (fs.existsSync(fixture.worktreePath)) {
        throw new Error(`worktree directory still exists after delete: ${fixture.worktreePath}`);
      }
      return "actual external git worktree directory, registration, and branch are gone";
    });

    await optionalStep(checks, "Keyboard shortcut smoke", async () => {
      await page.keyboard.press(process.platform === "darwin" ? "Meta+K" : "Control+K");
      await page.waitForTimeout(1000);
      await page.keyboard.press("Escape");
      await page.waitForTimeout(600);
      return "command/search shortcut did not crash renderer";
    });

    await optionalStep(checks, "Main page contains KanVibe UI text", async () => {
      const text = (await page.locator("body").innerText({ timeout: 5000 })).slice(0, 2000);
      if (!/KanVibe|TODO|PROGRESS|REVIEW|DONE|프로젝트|태스크|칸반/i.test(text)) {
        throw new Error(`expected KanVibe board/navigation text, got: ${text.slice(0, 160)}`);
      }
      return "board/navigation text detected";
    });

    await optionalStep(checks, "CDP runtime/network diagnostics captured", async () => {
      const metrics = cdpSession ? await cdpSession.send("Performance.getMetrics") : { metrics: [] };
      const metricNames = new Set((metrics.metrics || []).map((metric) => metric.name));
      if (!metricNames.has("Timestamp")) {
        throw new Error("CDP Performance metrics did not include Timestamp");
      }
      return `requests=${cdpCollector.counters.requests}, responses=${cdpCollector.counters.responses}, failures=${cdpCollector.counters.failedRequests}, exceptions=${cdpCollector.counters.exceptions}`;
    });

    await optionalStep(checks, "No blocking console errors", async () => {
      const blocking = consoleErrors.filter((line) => !/favicon|DevTools|Electron Security Warning|Insecure Content-Security-Policy/i.test(line));
      if (blocking.length > 0) throw new Error(blocking.join("\n"));
      return "none";
    });

  } catch (error) {
    checks.push({ name: "Electron QA flow", ok: false, detail: error instanceof Error ? error.stack || error.message : String(error) });
  } finally {
    if (page && traceStarted) {
      await page.context().tracing.stop({ path: run.tracePath }).catch((error) => {
        checks.push({ name: "Playwright trace artifact", ok: false, detail: error instanceof Error ? error.message : String(error) });
      });
      traceStarted = false;
    }
    writeJson(run.cdpDiagnosticsPath, cdpCollector.summary());
    if (cdpSession) await cdpSession.detach().catch(() => {});
    if (app) await app.close().catch(() => {});
  }

  await optionalStep(checks, "Playwright trace artifact written", async () => {
    if (!fs.existsSync(run.tracePath) || fs.statSync(run.tracePath).size === 0) {
      throw new Error(`trace missing or empty: ${run.tracePath}`);
    }
    return run.tracePath;
  });

  await optionalStep(checks, "CDP diagnostics artifact written", async () => {
    if (!fs.existsSync(run.cdpDiagnosticsPath) || fs.statSync(run.cdpDiagnosticsPath).size === 0) {
      throw new Error(`CDP diagnostics missing or empty: ${run.cdpDiagnosticsPath}`);
    }
    return run.cdpDiagnosticsPath;
  });

  await optionalStep(checks, "Isolated app-data database written inside run directory", async () => {
    const isolatedDatabasePath = path.join(run.runDir, "app-data", "kanvibe.db");
    if (!fs.existsSync(isolatedDatabasePath) || fs.statSync(isolatedDatabasePath).size === 0) {
      throw new Error(`isolated QA database missing or empty: ${isolatedDatabasePath}`);
    }
    return isolatedDatabasePath;
  });

  ok = checks.every((check) => check.ok);

  if (fs.existsSync(run.videoPath)) {
    notes.push("Actual X11 screen recording captured with ffmpeg.");
  } else {
    notes.push("No mp4 was present when smoke flow finished; wrapper script can synthesize one from screenshots.");
  }
  notes.push(`Fixture public repo: ${fixture.repositoryUrl}`);
  notes.push(`Fixture clone registered as project: ${fixture.projectName} / ${fixture.projectDir}`);
  notes.push(`Fixture branch/external worktree under test: ${fixture.branchName} / ${fixture.worktreePath}`);
  notes.push("QA uses an isolated KANVIBE_APP_DATA_DIR inside the run directory, not the user's app database.");
  diagnostics.push(`CDP counters: ${JSON.stringify(cdpCollector.counters)}`);
  diagnostics.push("Playwright trace captures actions, screenshots, DOM snapshots, console, and network timeline.");

  const result = {
    ok,
    scope: "Electron UI QA for PR #275/#276 cleanup plus Vim-style board controls: clone fixture repo, seed real tasks, verify h/j/k/l, n, dd, :move todo|progress|pending|review|done, settings ON/OFF behavior, and external worktree cleanup",
    branch: gitValue(["branch", "--show-current"]),
    commit: gitValue(["rev-parse", "--short", "HEAD"]),
    checks,
    errors: consoleErrors,
    diagnostics,
    screenshots,
    videoPath: fs.existsSync(run.videoPath) ? run.videoPath : null,
    tracePath: fs.existsSync(run.tracePath) ? run.tracePath : null,
    cdpDiagnosticsPath: fs.existsSync(run.cdpDiagnosticsPath) ? run.cdpDiagnosticsPath : null,
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
