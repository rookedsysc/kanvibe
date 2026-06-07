#!/usr/bin/env node
/* eslint-disable @typescript-eslint/no-require-imports */
const fs = require("node:fs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const { createQaRun } = require("../lib/report.cjs");
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

function writeJson(filePath, data) {
  fs.writeFileSync(filePath, `${JSON.stringify(data, null, 2)}\n`, "utf8");
}

function renderMarkdown(result) {
  const lines = [];
  lines.push("# KanVibe Move Command Autocomplete QA Report");
  lines.push("");
  lines.push(`- Run ID: \`${result.runId}\``);
  lines.push(`- Branch: \`${result.branch || "unknown"}\``);
  lines.push(`- Commit: \`${result.commit || "unknown"}\``);
  lines.push(`- Scope: ${result.scope}`);
  lines.push(`- Status: **${result.ok ? "PASS" : "FAIL"}**`);
  lines.push("");
  lines.push("## Checks");
  lines.push("");
  for (const check of result.checks || []) {
    lines.push(`- ${check.ok ? "✅" : "❌"} **${check.name}**${check.detail ? ` — ${check.detail}` : ""}`);
  }
  lines.push("");
  lines.push("## Console / Runtime Errors");
  lines.push("");
  if (result.errors?.length) {
    for (const error of result.errors) lines.push(`- ${error}`);
  } else {
    lines.push("No blocking console/runtime errors captured.");
  }
  lines.push("");
  lines.push("## Evidence");
  lines.push("");
  for (const shot of result.screenshots || []) lines.push(`- ${shot.label}: \`${shot.path}\``);
  if (result.videoPath) lines.push(`- Video: \`${result.videoPath}\``);
  lines.push("");
  lines.push("## Diagnostics");
  lines.push("");
  if (result.tracePath) lines.push(`- Playwright trace: \`${result.tracePath}\``);
  if (result.cdpDiagnosticsPath) lines.push(`- CDP diagnostics: \`${result.cdpDiagnosticsPath}\``);
  for (const diagnostic of result.diagnostics || []) lines.push(`- ${diagnostic}`);
  lines.push("");
  lines.push("## Notes");
  lines.push("");
  for (const note of result.notes || []) lines.push(`- ${note}`);
  lines.push("");
  return `${lines.join("\n")}\n`;
}

async function invokeDesktop(page, namespace, method, ...args) {
  return page.evaluate(
    async ({ namespace, method, args }) => window.kanvibeDesktop.invoke(namespace, method, args),
    { namespace, method, args },
  );
}

function taskCardSelector(taskId) {
  return `[data-kanban-task-card='true'][data-kanban-task-id='${taskId}']`;
}

async function takeScreenshot(page, run, label, screenshots) {
  const fileName = `${String(screenshots.length + 1).padStart(2, "0")}-${label}.png`;
  const shotPath = path.join(run.screenshotsDir, fileName);
  await page.screenshot({ path: shotPath, fullPage: true });
  screenshots.push({ label, path: shotPath });
}

async function dismissUpdateDialogIfPresent(page) {
  const closeButton = page.getByRole("button", { name: "닫기" }).first();
  try {
    await closeButton.click({ timeout: 2500 });
    await page.waitForTimeout(500);
  } catch {
    // Optional release/update dialog may not be present.
  }
}

async function navigateToBoard(page) {
  await page.evaluate(() => {
    window.location.hash = "#/ko";
  });
  await page.waitForTimeout(800);
  await dismissUpdateDialogIfPresent(page);
}

async function waitForTaskCard(page, taskId) {
  const card = page.locator(taskCardSelector(taskId));
  await card.waitFor({ state: "visible", timeout: 20000 });
  await card.scrollIntoViewIfNeeded();
  return card;
}

async function focusTaskCard(page, taskId) {
  const card = await waitForTaskCard(page, taskId);
  await card.focus();
  await page.waitForTimeout(500);
}

async function waitForTaskStatus(page, taskId, status) {
  await page.locator(`${taskCardSelector(taskId)}[data-kanban-status='${status}']`).waitFor({ state: "visible", timeout: 15000 });
  const task = await invokeDesktop(page, "kanban", "getTaskById", taskId);
  if (!task || task.status !== status) {
    throw new Error(`expected backend status ${status} for ${taskId}, got ${task ? task.status : "null"}`);
  }
  return task;
}

async function openVimCommand(page, taskId) {
  await focusTaskCard(page, taskId);
  await page.keyboard.press(":");
  const commandInput = page.locator("#vim-command-input");
  await commandInput.waitFor({ state: "visible", timeout: 7000 });
  await page.waitForTimeout(500);
  return commandInput;
}

async function setStepOverlay(page, text) {
  await page.evaluate((message) => {
    let el = document.getElementById("kanvibe-qa-step-overlay");
    if (!el) {
      el = document.createElement("div");
      el.id = "kanvibe-qa-step-overlay";
      el.style.position = "fixed";
      el.style.top = "16px";
      el.style.left = "50%";
      el.style.transform = "translateX(-50%)";
      el.style.zIndex = "9999";
      el.style.maxWidth = "980px";
      el.style.padding = "10px 14px";
      el.style.borderRadius = "999px";
      el.style.border = "1px solid rgba(90, 141, 255, 0.75)";
      el.style.background = "rgba(8, 12, 24, 0.92)";
      el.style.color = "#f8fafc";
      el.style.font = "600 16px/1.35 ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif";
      el.style.boxShadow = "0 10px 30px rgba(0, 0, 0, 0.35)";
      el.style.pointerEvents = "none";
      document.body.appendChild(el);
    }
    el.textContent = message;
  }, text);
}

function createCdpDiagnosticsCollector() {
  const events = [];
  const counters = { requests: 0, responses: 0, failedRequests: 0, exceptions: 0, logEntries: 0 };
  const pushEvent = (type, payload) => {
    if (events.length >= 200) return;
    events.push({ type, timestamp: new Date().toISOString(), payload });
  };
  return { events, counters, pushEvent, summary: () => ({ counters, sampledEvents: events }) };
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
  cdpSession.on("Network.requestWillBeSent", () => { collector.counters.requests += 1; });
  cdpSession.on("Network.responseReceived", () => { collector.counters.responses += 1; });
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
  const diagnostics = [];
  const notes = [];
  const cdpCollector = createCdpDiagnosticsCollector();
  let app;
  let page;
  let cdpSession;
  let traceStarted = false;

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
    const launched = await launchKanVibeElectron({
      rootDir: process.cwd(),
      outputDir: run.runDir,
      appDataDir: path.join(run.runDir, "app-data"),
    });
    app = launched.app;
    page = launched.page;

    page.on("console", (message) => {
      if (["error", "warning"].includes(message.type())) errors.push(`${message.type()}: ${message.text()}`);
    });
    page.on("pageerror", (error) => errors.push(`pageerror: ${error.message}`));
    page.on("requestfailed", (request) => {
      const failure = request.failure();
      errors.push(`requestfailed: ${request.url()} ${failure?.errorText || "unknown"}`);
    });

    await page.context().tracing.start({ screenshots: true, snapshots: true, sources: true });
    traceStarted = true;

    await check("CDP diagnostics session attaches", async () => {
      cdpSession = await attachCdpDiagnostics(page, cdpCollector);
      return `remote-debugging-port=${launched.cdpPort}`;
    });

    await check("Electron window opens with isolated app data", async () => {
      await page.waitForLoadState("domcontentloaded");
      await dismissUpdateDialogIfPresent(page);
      return `KANVIBE_APP_DATA_DIR=${path.join(run.runDir, "app-data")}`;
    });

    let task;
    await check("Seed isolated TODO task and enable Vim mode through app IPC", async () => {
      await invokeDesktop(page, "appSettings", "setVimModeEnabled", true);
      task = await invokeDesktop(page, "kanban", "createTask", {
        title: `move-autocomplete-qa-${run.runId}`,
        description: "Electron QA: validate Vim :move Tab autocomplete from the real board command input, then submit and assert UI/backend status.",
      });
      if (!task?.id) throw new Error(`Task creation failed: ${JSON.stringify(task)}`);
      await navigateToBoard(page);
      await page.reload({ waitUntil: "domcontentloaded" });
      await dismissUpdateDialogIfPresent(page);
      await waitForTaskStatus(page, task.id, "todo");
      return `task=${task.id} (${task.title})`;
    });

    await setStepOverlay(page, "KanVibe QA: seeded TODO task; opening Vim command with ':'");
    await takeScreenshot(page, run, "seeded-todo-task-visible", screenshots);
    await page.waitForTimeout(1200);

    await check("Ambiguous autocomplete prefix does not rewrite `move p`", async () => {
      await setStepOverlay(page, "Ambiguous prefix check: type 'move p' then Tab (progress/pending => no completion)");
      const input = await openVimCommand(page, task.id);
      await page.keyboard.type("move p", { delay: 80 });
      await page.waitForTimeout(500);
      await page.keyboard.press("Tab");
      await page.waitForTimeout(900);
      const value = await input.inputValue();
      if (value !== "move p") throw new Error(`expected ambiguous input to remain 'move p', got ${JSON.stringify(value)}`);
      const hintText = await page.locator("#vim-command-input").locator("xpath=ancestor::div[contains(@class,'fixed')][1]//p").innerText().catch(() => "");
      if (/move progress|move pending/i.test(hintText)) throw new Error(`ambiguous prefix unexpectedly showed completion hint: ${hintText}`);
      await input.click();
      await page.keyboard.press("Escape");
      await page.locator("#vim-command-input").waitFor({ state: "hidden", timeout: 7000 });
      return "Tab left `move p` unchanged and no progress/pending completion hint appeared";
    });

    await takeScreenshot(page, run, "ambiguous-move-p-not-autocompleted", screenshots);
    await page.waitForTimeout(1000);

    await check("Unique prefix `move re` autocompletes to `move review` on Tab", async () => {
      await setStepOverlay(page, "Unique prefix check: type 'move re'; hint offers ':move review'; Tab completes it");
      const input = await openVimCommand(page, task.id);
      await page.keyboard.type("move re", { delay: 90 });
      await page.getByText(/Tab을 누르면 :move review/).waitFor({ state: "visible", timeout: 7000 });
      await takeScreenshot(page, run, "completion-hint-move-review-visible", screenshots);
      await page.waitForTimeout(800);
      await page.keyboard.press("Tab");
      await page.waitForTimeout(900);
      const completedValue = await input.inputValue();
      if (completedValue !== "move review") throw new Error(`expected input 'move review' after Tab, got ${JSON.stringify(completedValue)}`);
      const activeId = await page.evaluate(() => document.activeElement?.id || null);
      if (activeId !== "vim-command-input") throw new Error(`expected command input to retain focus after autocomplete, got ${activeId}`);
      return "Tab completed `move re` to `move review` and kept command input focused";
    });

    await takeScreenshot(page, run, "tab-completed-to-move-review", screenshots);
    await page.waitForTimeout(1000);

    await check("Submitting completed command moves task to REVIEW in UI and backend", async () => {
      await setStepOverlay(page, "Submit completed command: Enter moves the focused task from TODO to REVIEW");
      await page.keyboard.press("Enter");
      await page.locator("#vim-command-input").waitFor({ state: "hidden", timeout: 7000 });
      const movedTask = await waitForTaskStatus(page, task.id, "review");
      await page.waitForTimeout(1200);
      return `UI card data-kanban-status=review; backend getTaskById(${task.id}).status=${movedTask.status}`;
    });

    await takeScreenshot(page, run, "task-moved-to-review-after-autocomplete-submit", screenshots);

    await check("CDP runtime/network diagnostics captured", async () => {
      const metrics = cdpSession ? await cdpSession.send("Performance.getMetrics") : { metrics: [] };
      const metricNames = new Set((metrics.metrics || []).map((metric) => metric.name));
      if (!metricNames.has("Timestamp")) throw new Error("CDP Performance metrics did not include Timestamp");
      return `requests=${cdpCollector.counters.requests}, responses=${cdpCollector.counters.responses}, failures=${cdpCollector.counters.failedRequests}, exceptions=${cdpCollector.counters.exceptions}`;
    });

    await check("No blocking console/runtime errors", async () => {
      const blocking = errors.filter((line) => !/favicon|DevTools|Electron Security Warning|Insecure Content-Security-Policy/i.test(line));
      if (blocking.length > 0) throw new Error(blocking.join("\n"));
      return "none";
    });
  } catch (error) {
    if (!checks.some((item) => item.name === "Electron move autocomplete QA flow" && !item.ok)) {
      checks.push({ name: "Electron move autocomplete QA flow", ok: false, detail: error instanceof Error ? (error.stack || error.message) : String(error) });
    }
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

  await check("Playwright trace artifact written", async () => {
    if (!fs.existsSync(run.tracePath) || fs.statSync(run.tracePath).size === 0) throw new Error(`trace missing or empty: ${run.tracePath}`);
    return run.tracePath;
  }).catch(() => {});

  await check("CDP diagnostics artifact written", async () => {
    if (!fs.existsSync(run.cdpDiagnosticsPath) || fs.statSync(run.cdpDiagnosticsPath).size === 0) throw new Error(`CDP diagnostics missing or empty: ${run.cdpDiagnosticsPath}`);
    return run.cdpDiagnosticsPath;
  }).catch(() => {});

  await check("Isolated app-data database written inside run directory", async () => {
    const dbPath = path.join(run.runDir, "app-data", "kanvibe.db");
    if (!fs.existsSync(dbPath) || fs.statSync(dbPath).size === 0) throw new Error(`isolated QA database missing or empty: ${dbPath}`);
    return dbPath;
  }).catch(() => {});

  const ok = checks.every((item) => item.ok);
  const result = {
    ok,
    runId: run.runId,
    scope: "Electron QA for KanVibe Vim :move command autocomplete: ambiguous status prefix remains unchanged, unique `move re` Tab-completes to `move review`, and Enter moves the focused task to REVIEW with UI + backend assertions.",
    branch: gitValue(["branch", "--show-current"]),
    commit: gitValue(["rev-parse", "--short", "HEAD"]),
    checks,
    errors,
    diagnostics: [
      `CDP counters: ${JSON.stringify(cdpCollector.counters)}`,
      "Playwright trace captures actions, screenshots, DOM snapshots, console, and network timeline.",
    ],
    screenshots,
    videoPath: fs.existsSync(run.videoPath) ? run.videoPath : null,
    tracePath: fs.existsSync(run.tracePath) ? run.tracePath : null,
    cdpDiagnosticsPath: fs.existsSync(run.cdpDiagnosticsPath) ? run.cdpDiagnosticsPath : null,
    notes: [
      "QA used an isolated KANVIBE_APP_DATA_DIR inside the run directory, not the user's app database.",
      "The on-screen QA overlay is injected only for recording readability; assertions use the real KanVibe UI and IPC backend state.",
    ],
  };

  writeJson(run.jsonPath, result);
  fs.writeFileSync(run.reportPath, renderMarkdown(result), "utf8");
  console.log(JSON.stringify({ ok, runDir: run.runDir, reportPath: run.reportPath, videoPath: run.videoPath }, null, 2));
  process.exit(ok ? 0 : 1);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
