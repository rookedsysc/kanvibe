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
    await optionalStep(checks, "Seed isolated QA project and task through app IPC", async () => {
      seededResult = await seedQaProjectAndTask(page, fixture);
      await page.reload({ waitUntil: "domcontentloaded" });
      await dismissUpdateDialogIfPresent(page);
      return `project=${seededResult.project.name}, task=${seededResult.task.id}, dbWorktreePath=${seededResult.task.worktreePath ?? "null"}`;
    });
    seeded = seededResult;

    if (!seeded) {
      throw new Error("QA seed failed; cannot continue UI flow");
    }

    await optionalStep(checks, "Board shows seeded QA task card", async () => {
      await waitForTaskCard(page, seeded.task.id);
      return fixture.branchName;
    });

    await page.waitForTimeout(1500);
    await takeScreenshot(page, run, "seeded-task-visible", screenshots);

    await optionalStep(checks, "UI context menu changes task status TODO to PROGRESS", async () => {
      await changeTaskStatusFromContextMenu(page, seeded.task.id, 1);
      await page.locator(`[data-kanban-task-card='true'][data-kanban-task-id='${seeded.task.id}'][data-kanban-status='progress']`).waitFor({ state: "visible", timeout: 10000 });
      const task = await invokeDesktop(page, "kanban", "getTaskById", seeded.task.id);
      if (task?.status !== "progress") {
        throw new Error(`expected backend status progress, got ${task?.status}`);
      }
      return "card moved to PROGRESS and backend status updated";
    });

    await takeScreenshot(page, run, "task-moved-to-progress", screenshots);
    await page.waitForTimeout(1500);

    await optionalStep(checks, "UI delete removes task record", async () => {
      await deleteTaskFromContextMenu(page, seeded.task.id);
      await page.waitForFunction(
        async (taskId) => {
          const task = await window.kanvibeDesktop.invoke("kanban", "getTaskById", [taskId]);
          return task === null;
        },
        seeded.task.id,
        { timeout: 15000 },
      );
      return "backend getTaskById returned null after confirm delete";
    });

    await page.reload({ waitUntil: "domcontentloaded" });
    await page.waitForTimeout(1500);
    await takeScreenshot(page, run, "task-deleted-after-refresh", screenshots);

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
    scope: "Electron UI QA for PR #275/#276: clone the public QA fixture repo, register a run-unique project, create/move/delete a task, and verify cleanup of a real external git worktree outside KanVibe's managed __worktrees convention when DB worktreePath is empty",
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
