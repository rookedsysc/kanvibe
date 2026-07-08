#!/usr/bin/env node
/* eslint-disable @typescript-eslint/no-require-imports */

const fs = require("node:fs");
const http = require("node:http");
const path = require("node:path");
const { spawn, spawnSync } = require("node:child_process");
const { chromium } = require("@playwright/test");

const rootDir = process.cwd();
const seedPath = path.join(rootDir, "qa", "seed", "kanvibe-seed.sqlite");
const scenariosDir = path.join(rootDir, "qa", "scenarios");
const baselineDir = path.join(rootDir, "qa", "baseline");
const screensDir = path.join(baselineDir, "screens");
const videosDir = path.join(baselineDir, "videos");
const runDir = path.join(baselineDir, ".run");
const appDataDir = path.join(runDir, "app-data");
const runtimeDbPath = path.join(runDir, "kanvibe.db");
const executablePath = path.join(rootDir, "dist", "linux-unpacked", "kanvibe");
const viewport = { width: 1440, height: 960 };

const visualStates = {
  "S01": { hash: "#/ko", waitFor: "board", note: "Board overview" },
  "S02": { hash: "#/ko", waitFor: "board", clickText: "새 작업", note: "Create task modal" },
  "S03": { hash: "#/ko/task/qa-task-progress-terminal", waitFor: "route", note: "Task detail terminal task" },
  "S04": { hash: "#/ko/task/qa-task-review-diff", waitFor: "route", note: "Task detail PR task" },
  "S05": { hash: "#/ko/task/qa-task-review-diff/diff", waitFor: "route", note: "Diff route" },
  "S06": { hash: "#/ko", waitFor: "board", focusTaskId: "qa-task-todo-local", note: "Branch-from-task source card" },
  "S07": { hash: "#/ko", waitFor: "board", focusTaskId: "qa-task-todo-local", note: "Board drag/drop source state" },
  "S08": { hash: "#/ko", waitFor: "board", focusTaskId: "qa-task-pending-no-branch", note: "Context menu/delete source state" },
  "S09": { hash: "#/ko", waitFor: "board", press: ":", note: "Vim command entry state" },
  "S10": { hash: "#/ko/settings", waitFor: "route", note: "Settings route" },
  "S11": { hash: "#/ko/task/qa-task-review-ai-history", waitFor: "route", note: "Hooks/AI task detail" },
  "S12": { hash: "#/ko", waitFor: "board", note: "Project filter source board" },
  "S13": { hash: "#/ko/task/qa-task-progress-pr", waitFor: "route", note: "Existing-window focus task detail" },
  "S14": { hash: "#/ko/task/qa-task-todo-remote", waitFor: "route", note: "Remote session task detail" },
};

function ensurePrerequisites() {
  if (!process.env.DISPLAY) {
    throw new Error("DISPLAY is required. Run with xvfb-run or a desktop display.");
  }
  if (!fs.existsSync(executablePath)) {
    throw new Error(`Packaged app not found: ${executablePath}. Run pnpm dist:dir first.`);
  }
  if (!fs.existsSync(seedPath)) {
    throw new Error(`QA seed not found: ${seedPath}. Run pnpm qa:seed first.`);
  }
}

function readScenarios() {
  return fs.readdirSync(scenariosDir)
    .filter((file) => /^S\d\d-.*\.json$/.test(file))
    .sort()
    .map((file) => {
      const scenario = JSON.parse(fs.readFileSync(path.join(scenariosDir, file), "utf8"));
      return { ...scenario, file };
    });
}

function wait(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function requestJson(port, requestPath) {
  return new Promise((resolve, reject) => {
    const request = http.get({ host: "127.0.0.1", port, path: requestPath }, (response) => {
      const chunks = [];
      response.on("data", (chunk) => chunks.push(chunk));
      response.on("end", () => {
        if (!response.statusCode || response.statusCode < 200 || response.statusCode >= 300) {
          reject(new Error(`GET ${requestPath} returned ${response.statusCode}`));
          return;
        }
        resolve(JSON.parse(Buffer.concat(chunks).toString("utf8")));
      });
    });
    request.on("error", reject);
    request.setTimeout(1000, () => request.destroy(new Error(`GET ${requestPath} timed out`)));
  });
}

async function waitForCdp(port, child, output) {
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    if (child.exitCode !== null || child.signalCode) {
      throw new Error(`Electron exited before CDP opened: exit=${child.exitCode} signal=${child.signalCode}\n${output()}`);
    }
    try {
      return await requestJson(port, "/json/version");
    } catch {
      await wait(100);
    }
  }
  throw new Error(`Timed out waiting for CDP\n${output()}`);
}

async function waitForFirstPage(browser) {
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    for (const context of browser.contexts()) {
      const page = context.pages()[0];
      if (page) return page;
    }
    await wait(100);
  }
  throw new Error("Timed out waiting for first page");
}

function collectOutput(child) {
  const chunks = [];
  child.stdout?.on("data", (chunk) => chunks.push(chunk.toString("utf8")));
  child.stderr?.on("data", (chunk) => chunks.push(chunk.toString("utf8")));
  return () => chunks.join("");
}

async function launchApp() {
  const port = 47000 + Math.floor(Math.random() * 1000);
  const child = spawn(executablePath, [`--remote-debugging-port=${port}`, "--no-sandbox"], {
    cwd: rootDir,
    env: {
      ...process.env,
      CI: "1",
      KANVIBE_QA_MODE: "1",
      KANVIBE_APP_DATA_DIR: appDataDir,
      KANVIBE_DB_PATH: runtimeDbPath,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const output = collectOutput(child);
  await waitForCdp(port, child, output);
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  const page = await waitForFirstPage(browser);
  page.setDefaultTimeout(15_000);
  await page.setViewportSize(viewport).catch(() => {});
  return { child, browser, page, output };
}

async function stopApp(app) {
  await app.browser.close().catch(() => {});
  if (app.child.exitCode === null && !app.child.signalCode) app.child.kill();
}

function startVideo(filePath) {
  return spawn("ffmpeg", [
    "-y",
    "-loglevel", "error",
    "-f", "x11grab",
    "-video_size", `${viewport.width}x${viewport.height}`,
    "-framerate", "15",
    "-i", process.env.DISPLAY,
    "-codec:v", "libx264",
    "-preset", "ultrafast",
    "-pix_fmt", "yuv420p",
    filePath,
  ], { stdio: ["ignore", "pipe", "pipe"] });
}

async function stopVideo(child) {
  if (child.exitCode !== null || child.signalCode) return;
  child.kill("SIGINT");
  await new Promise((resolve) => child.once("exit", resolve));
}

function createStillVideo(screenPath, videoPath) {
  const result = spawnSync("ffmpeg", [
    "-y",
    "-loglevel", "error",
    "-loop", "1",
    "-t", "2",
    "-i", screenPath,
    "-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2,format=yuv420p",
    "-movflags", "+faststart",
    videoPath,
  ], { encoding: "utf8" });

  if (result.status !== 0) {
    throw new Error(`Failed to create fallback video for ${screenPath}: ${result.stderr || result.stdout}`);
  }
}

async function waitForBoard(page) {
  await page.waitForFunction(
    () => !document.querySelector("[data-testid='board-route-skeleton']")
      && document.querySelectorAll("[data-rfd-droppable-id]").length >= 5,
    null,
    { timeout: 30_000 },
  );
}

async function waitForRoute(page) {
  await page.waitForFunction(() => window.kanvibeDesktop?.isDesktop === true, null, { timeout: 30_000 });
  await page.waitForLoadState("domcontentloaded").catch(() => {});
  await wait(1500);
}

async function prepareVisualState(page, scenario) {
  const prefix = scenario.id.slice(0, 3);
  const state = visualStates[prefix] || visualStates.S01;
  await page.evaluate((hash) => { window.location.hash = hash; }, state.hash);

  if (state.waitFor === "board") await waitForBoard(page);
  else await waitForRoute(page);

  if (state.focusTaskId) {
    await page.locator(`[data-kanban-task-card='true'][data-kanban-task-id='${state.focusTaskId}']`).focus().catch(() => {});
  }
  if (state.clickText) {
    await page.getByText(state.clickText, { exact: true }).first().click().catch(() => {});
    await wait(500);
  }
  if (state.press) {
    await page.keyboard.press(state.press).catch(() => {});
    await wait(500);
  }

  return state;
}

function resetBaselineDirs() {
  fs.rmSync(baselineDir, { recursive: true, force: true });
  fs.mkdirSync(screensDir, { recursive: true });
  fs.mkdirSync(videosDir, { recursive: true });
  fs.mkdirSync(appDataDir, { recursive: true });
  fs.copyFileSync(seedPath, runtimeDbPath);
}

function writeManifest(entries) {
  const lines = [
    "# Electron Baseline Manifest",
    "",
    `Captured: ${new Date().toISOString()}`,
    `Seed: \`qa/seed/kanvibe-seed.sqlite\` copied to \`${path.relative(rootDir, runtimeDbPath)}\``,
    `App: \`${path.relative(rootDir, executablePath)}\``,
    "",
    "| Scenario | Source | Screen | Video | Notes |",
    "| --- | --- | --- | --- | --- |",
  ];

  for (const entry of entries) {
    lines.push(`| \`${entry.id}\` | \`${entry.source}\` | \`${entry.screen}\` | \`${entry.video}\` | ${entry.note} |`);
  }
  lines.push("");

  fs.writeFileSync(path.join(baselineDir, "MANIFEST.md"), lines.join("\n"), "utf8");
}

async function main() {
  ensurePrerequisites();
  const scenarios = readScenarios();
  resetBaselineDirs();
  const app = await launchApp();
  const entries = [];

  try {
    for (const scenario of scenarios) {
      const id = scenario.id.slice(0, 3);
      const screen = `screens/${scenario.id}.png`;
      const video = `videos/${scenario.id}.mp4`;
      const screenPath = path.join(baselineDir, screen);
      const videoPath = path.join(baselineDir, video);
      const recorder = startVideo(videoPath);
      const state = await prepareVisualState(app.page, scenario);
      await wait(1000);
      await app.page.screenshot({ path: screenPath, fullPage: true });
      await wait(500);
      await stopVideo(recorder);
      if (!fs.existsSync(videoPath) || fs.statSync(videoPath).size === 0) {
        createStillVideo(screenPath, videoPath);
      }
      entries.push({ id, source: scenario.file, screen, video, note: state.note });
    }
  } finally {
    await stopApp(app);
  }

  writeManifest(entries);
  console.log(`[qa:baseline] Captured ${entries.length} scenario baselines in ${baselineDir}`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
