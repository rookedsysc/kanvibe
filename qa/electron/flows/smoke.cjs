#!/usr/bin/env node
const fs = require("node:fs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const { createQaRun, writeReport } = require("../lib/report.cjs");
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

async function takeScreenshot(page, run, label, screenshots) {
  const fileName = `${String(screenshots.length + 1).padStart(2, "0")}-${label}.png`;
  const shotPath = path.join(run.screenshotsDir, fileName);
  await page.screenshot({ path: shotPath, fullPage: true });
  screenshots.push({ label, path: shotPath });
}

async function optionalStep(checks, name, fn) {
  try {
    const detail = await fn();
    checks.push({ name, ok: true, detail: detail || "ok" });
  } catch (error) {
    checks.push({ name, ok: false, detail: error instanceof Error ? error.message : String(error) });
  }
}

async function main() {
  const args = parseArgs(process.argv);
  const run = createQaRun({
    rootDir: process.cwd(),
    outputRoot: args.outputRoot,
    runId: args.runId,
    runDir: args.runDir,
  });

  const consoleErrors = [];
  const checks = [];
  const screenshots = [];
  const notes = [];
  let app;
  let ok = false;

  try {
    const launched = await launchKanVibeElectron({ rootDir: process.cwd(), outputDir: run.runDir });
    app = launched.app;
    const page = launched.page;

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
      return page.url();
    });

    await optionalStep(checks, "Renderer becomes visible", async () => {
      await page.locator("body").waitFor({ state: "visible", timeout: 20000 });
      return "body visible";
    });

    await takeScreenshot(page, run, "initial-render", screenshots);

    await optionalStep(checks, "Keyboard shortcut smoke", async () => {
      await page.keyboard.press(process.platform === "darwin" ? "Meta+K" : "Control+K");
      await page.waitForTimeout(500);
      await page.keyboard.press("Escape");
      return "command/search shortcut did not crash renderer";
    });

    await optionalStep(checks, "Mouse click smoke", async () => {
      await page.mouse.move(240, 180);
      await page.mouse.click(240, 180);
      await page.waitForTimeout(300);
      return "click accepted";
    });

    await takeScreenshot(page, run, "after-keyboard-mouse", screenshots);

    await optionalStep(checks, "Main page contains KanVibe UI text", async () => {
      const text = (await page.locator("body").innerText({ timeout: 5000 })).slice(0, 2000);
      if (!/KanVibe|TODO|PROGRESS|REVIEW|DONE|프로젝트|태스크|칸반/i.test(text)) {
        throw new Error(`expected KanVibe board/navigation text, got: ${text.slice(0, 160)}`);
      }
      return "board/navigation text detected";
    });

    await optionalStep(checks, "No blocking console errors", async () => {
      const blocking = consoleErrors.filter((line) => !/favicon|DevTools|Electron Security Warning|Insecure Content-Security-Policy/i.test(line));
      if (blocking.length > 0) throw new Error(blocking.join("\n"));
      return "none";
    });

    ok = checks.every((check) => check.ok);
  } catch (error) {
    checks.push({ name: "Electron QA flow", ok: false, detail: error instanceof Error ? error.stack || error.message : String(error) });
  } finally {
    if (app) await app.close().catch(() => {});
  }

  if (fs.existsSync(run.videoPath)) {
    notes.push("Actual X11 screen recording captured with ffmpeg.");
  } else {
    notes.push("No mp4 was present when smoke flow finished; wrapper script can synthesize one from screenshots.");
  }

  const result = {
    ok,
    scope: "Electron smoke QA plus PR #275/#276 resource-cleanup regression readiness",
    branch: gitValue(["branch", "--show-current"]),
    commit: gitValue(["rev-parse", "--short", "HEAD"]),
    checks,
    errors: consoleErrors,
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
