#!/usr/bin/env node
/* eslint-disable @typescript-eslint/no-require-imports */
const assert = require("node:assert/strict");
const fs = require("node:fs");
const http = require("node:http");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const { createQaRun } = require("../lib/report.cjs");
const { launchKanVibeElectron } = require("../lib/launchElectron.cjs");
const messages = require("../../../messages/ko.json");

function parseArgs(argv) {
  const options = {};
  for (let index = 2; index < argv.length; index += 1) {
    if (argv[index] === "--run-dir") options.runDir = path.resolve(argv[++index]);
    else if (argv[index] === "--run-id") options.runId = argv[++index];
    else if (argv[index] === "--serve-mobile") options.serveMobile = true;
  }
  return options;
}

function invoke(page, namespace, method, ...args) {
  return page.evaluate(({ namespace, method, args }) => window.kanvibeDesktop.invoke(namespace, method, args), { namespace, method, args });
}

function runtime(app, action) {
  return app.evaluate(async (_, action) => {
    const qa = globalThis.__backgroundSyncQa;
    if (!qa) throw new Error("The isolated sync fixture was not installed");
    if (action === "release") {
      const release = qa.release;
      if (!release) throw new Error("No stage is waiting for release");
      qa.release = null;
      release();
    }
    if (action === "release-delete") {
      if (!qa.releaseDelete) throw new Error("No deletion is waiting for release");
      qa.releaseDelete();
      qa.releaseDelete = null;
    }
    if (action === "no-review") qa.withReview = false;
    return { status: qa.sync.getBackgroundTaskSyncStatus(), counts: { ...qa.counts }, deleteCounts: { ...qa.deleteCounts }, pendingStage: qa.pendingStage };
  }, action);
}

async function installFixture(app, rootDir, appDataDir, tasks, longPath) {
  return app.evaluate(({ app: electronApp }, config) => {
    if (electronApp.getPath("userData") !== config.appDataDir) throw new Error("Isolated app-data ownership mismatch");
    const { createRequire } = process.getBuiltinModule("module");
    const load = createRequire(`${config.rootDir}/package.json`);
    const built = `${config.rootDir}/build/main/src`;
    const projects = load(`${built}/desktop/main/services/projectService.js`);
    const kanban = load(`${built}/desktop/main/services/kanbanService.js`);
    const sync = load(`${built}/desktop/main/services/backgroundTaskSyncService.js`);
    const registry = load(`${built}/desktop/main/serviceRegistry.js`);
    if (registry.desktopServices.backgroundTaskSync.getBackgroundTaskSyncStatus !== sync.getBackgroundTaskSyncStatus) {
      throw new Error("Fixture and IPC must load the same main-process service");
    }
    for (const [owner, name] of [[projects, "syncRegisteredProjectWorktrees"], [kanban, "syncActiveTaskPullRequests"], [kanban, "syncActiveTaskPulls"], [kanban, "deleteTask"]]) {
      if (!Object.getOwnPropertyDescriptor(owner, name)?.writable) throw new Error(`Main adapter cannot be controlled: ${name}`);
    }
    const originalDelete = kanban.deleteTask;
    const qa = {
      sync, counts: { worktrees: 0, pullRequests: 0, pulls: 0 }, deleteCounts: {},
      release: null, releaseDelete: null, pendingStage: null, withReview: true,
    };
    const barrier = (stage, value) => new Promise((resolve) => {
      qa.pendingStage = stage;
      qa.release = () => { qa.pendingStage = null; resolve(value); };
    });
    projects.syncRegisteredProjectWorktrees = () => {
      qa.counts.worktrees += 1;
      return barrier("worktrees", {
        worktreeTasks: [], hooksSetup: [], errors: [], changed: false,
        registeredWorktrees: qa.withReview ? [{ taskId: config.tasks[0].id, projectName: "KanVibe Sync QA", branchName: "qa/background-sync", worktreePath: config.longPath, sshHost: null }] : [],
      });
    };
    kanban.syncActiveTaskPullRequests = () => {
      qa.counts.pullRequests += 1;
      return barrier("pull-requests", {
        updatedTaskIds: [], mergeEventKeys: [],
        mergedPullRequests: qa.withReview ? config.tasks.map((task, index) => ({
          taskId: task.id, taskTitle: task.title, branchName: `qa/merged-${index + 1}`,
          prUrl: `https://github.com/kanvibe/qa-fixture/pull/${index + 1}`, mergedAt: "2026-11-14T00:00:00Z",
        })) : [],
      });
    };
    kanban.syncActiveTaskPulls = () => {
      qa.counts.pulls += 1;
      return barrier("pulls", { pulledTasks: qa.withReview ? [{
        taskId: config.tasks[1].id, taskTitle: config.tasks[1].title, branchName: "qa/merged-2", worktreePath: config.longPath,
        sshHost: null, status: "failed", summary: "QA remote is temporarily unavailable",
      }] : [] });
    };
    kanban.deleteTask = async (taskId) => {
      qa.deleteCounts[taskId] = (qa.deleteCounts[taskId] ?? 0) + 1;
      if (taskId === config.tasks[0].id && qa.deleteCounts[taskId] === 1) {
        await new Promise((resolve) => { qa.releaseDelete = resolve; });
      }
      if (taskId === config.tasks[1].id && qa.deleteCounts[taskId] === 1) throw new Error("QA worktree is busy");
      return originalDelete(taskId);
    };
    globalThis.__backgroundSyncQa = qa;
    return { pid: process.pid, appDataDir: electronApp.getPath("userData"), mutableAdapters: true, sameIpcService: true };
  }, { rootDir, appDataDir, tasks, longPath });
}

async function serveProbe(app, page, fixture) {
  const server = http.createServer((request, response) => {
    void (async () => {
      const route = new URL(request.url, "http://127.0.0.1").pathname;
      let result;
      if (route === "/health") result = { runId: fixture.runId, pid: process.pid, appDataDir: fixture.appDataDir };
      else if (route === "/status") result = await runtime(app);
      else if (request.method === "POST" && route === "/pairing") result = { ...await invoke(page, "mobileBridge", "startMobilePairing"), mobilePort: fixture.mobilePort };
      else if (request.method === "POST" && route === "/start") result = { status: await invoke(page, "backgroundTaskSync", "startBackgroundTaskSyncNow") };
      else if (request.method === "POST" && route === "/release") result = await runtime(app, "release");
      else { response.writeHead(404).end(); return; }
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify({ runId: fixture.runId, ...result }));
    })().catch((error) => {
      response.writeHead(500, { "content-type": "application/json" });
      response.end(JSON.stringify({ error: error.message }));
    });
  });
  await new Promise((resolve, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", resolve); });
  return server;
}

async function main() {
  const options = parseArgs(process.argv);
  const run = createQaRun({ ...options, rootDir: process.cwd() });
  const checks = [];
  const screenshots = [];
  const errors = [];
  let app;
  let page;
  let probe;
  let traceStarted = false;
  const appDataDir = path.join(run.runDir, "app-data");
  const diff = execFileSync("git", ["diff", "HEAD", "--binary"], { cwd: run.rootDir });
  const fingerprint = execFileSync("git", ["hash-object", "--stdin"], { cwd: run.rootDir, input: diff, encoding: "utf8" }).trim();
  const head = execFileSync("git", ["rev-parse", "HEAD"], { cwd: run.rootDir, encoding: "utf8" }).trim();
  const check = (name, detail) => checks.push({ name, ok: true, detail });
  const screenshot = async (label) => {
    const file = path.join(run.screenshotsDir, `${screenshots.length + 1}-${label}.png`);
    await page.screenshot({ path: file });
    screenshots.push({ label, path: file });
  };
  const stop = async () => {
    probe?.closeAllConnections();
    probe?.close();
    if (app) {
      fs.writeFileSync(path.join(run.runDir, "electron.log"), app.output());
      await app.close();
      app = null;
    }
  };
  try {
    const launched = await launchKanVibeElectron({ rootDir: run.rootDir, outputDir: run.runDir, appDataDir, viewport: { width: 1440, height: 960 }, actionTimeoutMs: 15000 });
    ({ app, page } = launched);
    assert.equal(await app.evaluate(({ app: electronApp }) => electronApp.getPath("userData")), appDataDir);
    page.on("pageerror", (error) => errors.push(error.message));
    await page.context().tracing.start({ screenshots: true, snapshots: true, sources: true });
    traceStarted = true;
    await page.waitForFunction(() => Boolean(window.kanvibeDesktop?.invoke));
    await invoke(page, "appSettings", "setBackgroundSyncEnabled", false);
    await invoke(page, "appSettings", "setNotificationEnabled", false);
    await page.evaluate(() => { window.location.hash = "#/ko"; });
    try { await page.getByRole("button", { name: /닫기|Close|关闭/, exact: true }).first().click({ timeout: 1200 }); } catch { /* 첫 실행 업데이트 안내만 닫는다. */ }
    const tasks = [];
    for (const title of ["QA merged task A", "QA merged task B"]) tasks.push(await invoke(page, "kanban", "createTask", { title }));
    const longPath = `/isolated/background-sync/${"long-worktree-directory/".repeat(12)}feature`;
    const identity = await installFixture(app, run.rootDir, appDataDir, tasks, longPath);
    check("main fixture identity and writable provider boundary", JSON.stringify(identity));
    const bell = () => page.getByRole("button", { name: messages.common.notifications, exact: true });
    const syncSection = () => page.getByRole("region", { name: messages.common.backgroundSyncStatus.title });
    await bell().click();
    await syncSection().getByRole("button", { name: messages.common.backgroundSyncStatus.start, exact: true }).click();
    await page.getByText(messages.common.backgroundSyncStatus.worktrees, { exact: true }).waitFor();
    assert.equal((await runtime(app)).status.phase, "worktrees");
    await invoke(page, "backgroundTaskSync", "startBackgroundTaskSyncNow");
    assert.equal((await runtime(app)).counts.worktrees, 1);
    assert.equal(await syncSection().getByRole("button", { name: messages.common.backgroundSyncStatus.running, exact: true }).isDisabled(), true);
    await screenshot("desktop-worktrees-running");
    check("real IPC starts one cycle and disables repeated UI starts", "worktree adapter called once after duplicate start");
    await bell().click();
    await bell().click();
    await page.getByText(messages.common.backgroundSyncStatus.worktrees, { exact: true }).waitFor();
    await runtime(app, "release");
    await page.getByText(messages.common.backgroundSyncStatus["pull-requests"], { exact: true }).waitFor();
    await runtime(app, "release");
    await page.getByText(messages.common.backgroundSyncStatus.pulls, { exact: true }).waitFor();
    await screenshot("desktop-pulls-running");
    await runtime(app, "release");
    await page.getByText(messages.common.backgroundSyncStatus.reviewNeeded, { exact: true }).waitFor();
    assert.deepEqual((await runtime(app)).counts, { worktrees: 1, pullRequests: 1, pulls: 1 });
    check("progress survives panel reopen and each service stage completes once", "worktrees → pull-requests → pulls → completed");
    const review = page.getByRole("button", { name: new RegExp(messages.common.backgroundSyncReview.title) });
    await review.click();
    await page.getByRole("heading", { name: messages.common.backgroundSyncReview.mergedSection, exact: true }).waitFor();
    const panel = page.getByRole("dialog", { name: messages.common.notifications, exact: true });
    assert.equal(await panel.locator("[aria-modal='true']").count(), 0);
    await screenshot("desktop-inline-review");
    await page.setViewportSize({ width: 390, height: 844 });
    const overflow = await panel.evaluate((element) => ({ width: element.clientWidth, scroll: element.scrollWidth, left: element.getBoundingClientRect().left, right: element.getBoundingClientRect().right, viewport: window.innerWidth }));
    assert.ok(overflow.scroll <= overflow.width + 1 && overflow.left >= 0 && overflow.right <= overflow.viewport, JSON.stringify(overflow));
    await screenshot("narrow-inline-review");
    await page.setViewportSize({ width: 1440, height: 960 });
    check("review stays inside notification panel and long paths fit at 390px", JSON.stringify(overflow));
    await panel.getByRole("button", { name: messages.common.delete, exact: true }).click();
    await panel.getByRole("status").filter({ hasText: "2개 중" }).waitFor();
    await bell().click();
    await runtime(app, "release-delete");
    await bell().click();
    await panel.getByRole("alert").filter({ hasText: "2개 중 1개 삭제, 1개 실패" }).waitFor();
    await screenshot("desktop-partial-delete-retained");
    await panel.getByRole("button", { name: messages.common.backgroundSyncReview.retryFailedDeletes, exact: true }).click();
    await page.getByRole("heading", { name: messages.common.notifications, exact: true }).waitFor();
    const deleted = await runtime(app);
    assert.equal(deleted.deleteCounts[tasks[0].id], 1);
    assert.equal(deleted.deleteCounts[tasks[1].id], 2);
    check("closing review preserves partial deletion and retries only the failed task", JSON.stringify(deleted.deleteCounts));
    await runtime(app, "no-review");
    await syncSection().getByRole("button", { name: messages.common.backgroundSyncStatus.start, exact: true }).click();
    for (const phase of ["worktrees", "pull-requests", "pulls"]) {
      await page.getByText(messages.common.backgroundSyncStatus[phase], { exact: true }).waitFor();
      await runtime(app, "release");
    }
    await page.getByText(messages.common.backgroundSyncStatus.noChanges, { exact: true }).waitFor();
    await screenshot("desktop-no-changes-completed");
    assert.deepEqual((await runtime(app)).counts, { worktrees: 2, pullRequests: 2, pulls: 2 });
    check("completed sync can run again and reports no changes", "two complete cycles, each provider called twice");
    assert.equal(errors.length, 0, errors.join("\n"));
    await page.context().tracing.stop({ path: run.tracePath });
    traceStarted = false;
    const result = { ok: true, runId: run.runId, head, fingerprint, checks, screenshots, errors, tracePath: run.tracePath, adapterBoundary: "External Git/PR providers controlled in isolated main; service orchestration, IPC, events, SQLite notification storage, and UI are real." };
    fs.writeFileSync(run.jsonPath, `${JSON.stringify(result, null, 2)}\n`);
    fs.writeFileSync(run.reportPath, `# Background sync Electron QA\n\n${checks.map((entry) => `- PASS ${entry.name}: ${entry.detail}`).join("\n")}\n\nArtifacts: ${run.jsonPath}\n`);
    if (options.serveMobile) {
      const fixture = { runId: run.runId, appDataDir, mobilePort: launched.hookPort + 10, identity, baseline: await runtime(app) };
      probe = await serveProbe(app, page, fixture);
      fixture.probeUrl = `http://127.0.0.1:${probe.address().port}`;
      fs.writeFileSync(path.join(run.runDir, "fixture.json"), `${JSON.stringify(fixture, null, 2)}\n`);
      fs.writeFileSync(path.join(run.runDir, "ready"), `${fixture.probeUrl}\n`);
      process.once("SIGTERM", () => { void stop().finally(() => process.exit(0)); });
      process.once("SIGINT", () => { void stop().finally(() => process.exit(0)); });
      console.log(`Desktop QA passed; mobile fixture ready at ${fixture.probeUrl}`);
      return;
    }
    console.log(`PASS ${checks.length} checks; ${run.jsonPath}`);
  } catch (error) {
    if (page) await screenshot("failure").catch(() => {});
    if (traceStarted) await page.context().tracing.stop({ path: run.tracePath }).catch(() => {});
    fs.writeFileSync(run.jsonPath, `${JSON.stringify({ ok: false, runId: run.runId, head, checks, screenshots, errors: [...errors, error.stack] }, null, 2)}\n`);
    throw error;
  } finally {
    if (!probe) await stop();
  }
}

main().catch((error) => { console.error(error.stack); process.exitCode = 1; });
