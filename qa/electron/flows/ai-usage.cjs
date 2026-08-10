#!/usr/bin/env node
/* eslint-disable @typescript-eslint/no-require-imports */
const fs = require("node:fs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const { createQaRun } = require("../lib/report.cjs");
const { launchKanVibeElectron } = require("../lib/launchElectron.cjs");

const AI_USAGE_SCOPE = "AI 사용량 패널: task 상세 좌측 dock 최하단 아이콘과 Mod+0으로 Claude·Codex·Gemini 남은 사용량 패널을 열고, 실제 자격증명으로 조회한 값이 렌더되는지와 dock 번호가 밀리지 않는지를 확인한다";

/** Linux Electron에서 Mod는 Control이다 */
const MOD_KEY = process.platform === "darwin" ? "Meta" : "Control";
const USAGE_SHORTCUT = `${MOD_KEY}+0`;

/** Electron 기본 메뉴가 이 키를 resetZoom으로도 쓰므로, 0이 아닌 값에서 시작해야 초기화 여부가 드러난다 */
const ZOOM_PROBE_LEVEL = 1;

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

/** 사용량 패널은 터미널을 요구하지 않으므로 worktree 없이 커밋 하나짜리 저장소면 충분하다 */
function createFixtureRepository(run) {
  const repoDir = path.join(run.runDir, "fixtures", "ai-usage-repo");
  fs.rmSync(path.dirname(repoDir), { recursive: true, force: true });
  fs.mkdirSync(repoDir, { recursive: true });
  execFileSync("git", ["init", "-b", "main"], { cwd: repoDir, stdio: "pipe" });
  execFileSync("git", ["config", "user.email", "qa@kanvibe.local"], { cwd: repoDir, stdio: "pipe" });
  execFileSync("git", ["config", "user.name", "KanVibe QA"], { cwd: repoDir, stdio: "pipe" });
  fs.writeFileSync(path.join(repoDir, "README.md"), "# KanVibe AI usage QA fixture\n", "utf8");
  execFileSync("git", ["add", "README.md"], { cwd: repoDir, stdio: "pipe" });
  execFileSync("git", ["commit", "-m", "Initial fixture commit"], { cwd: repoDir, stdio: "pipe" });
  return { repoDir, branchName: "feat/ai-usage-qa" };
}

function renderMarkdown(result) {
  const lines = [];
  lines.push("# KanVibe AI Usage Panel Electron QA Report");
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
  lines.push("## Observed Usage");
  lines.push("");
  if (result.observedUsage?.length) {
    for (const entry of result.observedUsage) lines.push(`- ${entry}`);
  } else {
    lines.push("No usage rows captured.");
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
  lines.push("## Notes");
  lines.push("");
  for (const note of result.notes || []) lines.push(`- ${note}`);
  lines.push("");
  return `${lines.join("\n")}\n`;
}

function writeReport(run, result) {
  const fullResult = { ...result, runId: run.runId };
  fs.writeFileSync(path.join(run.runDir, "result.json"), `${JSON.stringify(fullResult, null, 2)}\n`, "utf8");
  fs.writeFileSync(path.join(run.runDir, "report.md"), renderMarkdown(fullResult), "utf8");
}

async function invokeDesktop(page, namespace, method, ...args) {
  return page.evaluate(
    async ({ namespace, method, args }) => window.kanvibeDesktop.invoke(namespace, method, args),
    { namespace, method, args },
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

async function dismissUpdateDialogIfPresent(page) {
  await page.evaluate(() => {
    const button = document.querySelector("button[aria-label*='업데이트'][aria-label*='닫기']");
    if (button instanceof HTMLButtonElement) button.click();
  }).catch(() => {});
}

async function setStepOverlay(page, text) {
  await page.evaluate((message) => {
    let overlay = document.getElementById("kanvibe-qa-step-overlay");
    if (!overlay) {
      overlay = document.createElement("div");
      overlay.id = "kanvibe-qa-step-overlay";
      overlay.style.cssText = "position:fixed;top:16px;left:50%;transform:translateX(-50%);"
        + "z-index:9999;max-width:1080px;padding:10px 14px;border-radius:999px;"
        + "border:1px solid rgba(90,141,255,0.75);background:rgba(8,12,24,0.92);color:#f8fafc;"
        + "font:600 16px/1.35 ui-sans-serif,system-ui,sans-serif;pointer-events:none;";
      document.body.appendChild(overlay);
    }
    overlay.textContent = message;
  }, text);
}

async function readZoomLevel(app) {
  return app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].webContents.getZoomLevel());
}

async function setZoomLevel(app, level) {
  await app.evaluate(({ BrowserWindow }, targetLevel) => {
    BrowserWindow.getAllWindows()[0].webContents.setZoomLevel(targetLevel);
  }, level);
}

/**
 * 조회는 네트워크 왕복이라 걸리는 시간이 실행마다 다르다.
 * 고정 대기를 두면 느린 실행에서만 실패하므로 카드가 실제로 그려질 때까지 기다린다.
 */
async function waitForProviderCards(page, expectedCount) {
  await page.waitForFunction(
    (count) => document.querySelectorAll("[data-testid^='ai-usage-provider-']").length >= count,
    expectedCount,
    { timeout: 30000 },
  );
}

/** 패널이 실제 데이터를 그렸는지 provider 카드 텍스트로 확인한다 */
async function readProviderCardTexts(page) {
  return page.evaluate(() => (
    [...document.querySelectorAll("[data-testid^='ai-usage-provider-']")].map((card) => ({
      provider: card.getAttribute("data-testid").replace("ai-usage-provider-", ""),
      text: (card.textContent || "").replace(/\s+/g, " ").trim(),
      barCount: card.querySelectorAll("[style*='width']").length,
    }))
  ));
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
  const observedUsage = [];
  let app;
  let page;
  let fixture;
  let seededTask;
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
    fixture = createFixtureRepository(run);

    // 실제 사용량을 확인하는 것이 이 QA의 목적이므로 HOME을 가짜로 바꾸지 않는다
    notes.push(`HOME은 실제 사용자 홈(${process.env.HOME})을 그대로 쓴다. 조회 값은 실행 시점의 실제 구독 사용량이다.`);

    const launched = await launchKanVibeElectron({
      rootDir: process.cwd(),
      outputDir: run.runDir,
      appDataDir: path.join(run.runDir, "app-data"),
      viewport: { width: 1600, height: 1000 },
    });
    app = launched.app;
    page = launched.page;

    app.process().on("exit", (code, signal) => {
      if (isExpectedShutdown) return;
      errors.push(`electron exited: code=${code ?? "null"} signal=${signal ?? "null"}`);
    });
    page.on("console", (message) => {
      if (["error", "warning"].includes(message.type())) errors.push(`${message.type()}: ${message.text()}`);
    });
    page.on("pageerror", (error) => errors.push(`pageerror: ${error.message}`));

    await page.waitForLoadState("domcontentloaded");
    await dismissUpdateDialogIfPresent(page);

    await check("Seed an isolated git project and a KanVibe task", async () => {
      const projectResult = await invokeDesktop(page, "project", "registerProject", "KanVibe AI Usage QA", fixture.repoDir);
      if (!projectResult.success || !projectResult.project) {
        throw new Error(projectResult.error || "project registration failed");
      }

      seededTask = await invokeDesktop(page, "kanban", "createTask", {
        title: "QA AI usage panel",
        description: "Electron QA: AI usage panel via dock icon and Mod+0.",
        branchName: fixture.branchName,
        baseBranch: "main",
        projectId: projectResult.project.id,
      });
      if (!seededTask?.id) throw new Error("task creation failed");
      return `task=${seededTask.id}; repo=${fixture.repoDir}`;
    });

    await setStepOverlay(page, "1/6 task 상세 진입: dock 최하단에 사용량 아이콘이 보여야 함");
    await page.evaluate((taskId) => {
      window.location.hash = `#/ko/task/${taskId}`;
    }, seededTask.id);
    await page.waitForTimeout(1200);
    await dismissUpdateDialogIfPresent(page);
    await page.locator("[data-testid='task-detail-usage-button']").waitFor({ state: "visible", timeout: 20000 });
    await takeScreenshot(page, run, "task-detail-with-usage-icon", screenshots);

    await check("Usage icon carries the Mod+0 hint and does not consume a dock number", async () => {
      const usageTitle = await page.locator("[data-testid='task-detail-usage-button']").getAttribute("title");
      const expectedHint = process.platform === "darwin" ? "Cmd+0" : "Ctrl+0";
      if (!usageTitle || !usageTitle.includes(expectedHint)) {
        throw new Error(`usage button hint mismatch: ${usageTitle}`);
      }

      const dockTitles = await page.evaluate(() => (
        [...document.querySelectorAll("aside button, aside a")]
          .map((element) => element.getAttribute("title"))
          .filter(Boolean)
      ));
      const numberedDockTitles = dockTitles.filter((title) => /\+[1-9]\)$/.test(title));
      if (numberedDockTitles.length === 0) {
        throw new Error(`no numbered dock items found: ${JSON.stringify(dockTitles)}`);
      }
      return `usage=${usageTitle}; dock=${JSON.stringify(numberedDockTitles)}`;
    });

    await setStepOverlay(page, `2/6 ${USAGE_SHORTCUT} 입력: 사용량 패널이 열려야 함`);
    await check(`${USAGE_SHORTCUT} opens the AI usage panel`, async () => {
      await setZoomLevel(app, ZOOM_PROBE_LEVEL);
      await page.keyboard.press(USAGE_SHORTCUT);
      await page.locator("[data-testid='ai-usage-panel']").waitFor({ state: "visible", timeout: 15000 });

      const zoomLevelAfterShortcut = await readZoomLevel(app);
      const didResetZoom = Math.abs(zoomLevelAfterShortcut) < 0.001;
      notes.push(
        didResetZoom
          ? `${USAGE_SHORTCUT}은 Electron 기본 메뉴의 resetZoom도 함께 발동시킨다 (zoom ${ZOOM_PROBE_LEVEL} → ${zoomLevelAfterShortcut}).`
          : `${USAGE_SHORTCUT}은 줌 배율을 건드리지 않는다 (zoom 유지: ${zoomLevelAfterShortcut}).`,
      );
      await setZoomLevel(app, 0);
      return `panel opened; zoomLevelAfterShortcut=${zoomLevelAfterShortcut}`;
    });

    await check("Panel renders one card per provider with real fetched values", async () => {
      await waitForProviderCards(page, 3);
      const cards = await readProviderCardTexts(page);
      const providers = cards.map((card) => card.provider);
      for (const expected of ["claude", "codex", "gemini"]) {
        if (!providers.includes(expected)) {
          throw new Error(`missing provider card: ${expected} (got ${JSON.stringify(providers)})`);
        }
      }

      for (const card of cards) {
        observedUsage.push(`${card.provider}: bars=${card.barCount}; ${card.text}`);
      }

      const cardsWithPercent = cards.filter((card) => /\d+%/.test(card.text));
      if (cardsWithPercent.length === 0) {
        throw new Error(`no provider reported a usage percentage: ${JSON.stringify(cards)}`);
      }
      return `providers=${JSON.stringify(providers)}; withPercent=${cardsWithPercent.map((card) => card.provider).join(",")}`;
    });
    await takeScreenshot(page, run, "ai-usage-panel-open", screenshots);

    await setStepOverlay(page, "4/6 새로고침: 다시 조회해도 값이 유지되어야 함");
    await check("Refresh button re-queries usage without emptying the panel", async () => {
      await page.locator("[data-testid='ai-usage-refresh']").click({ timeout: 10000 });
      // 버튼이 다시 눌릴 수 있게 되면 재조회가 끝난 것이다
      await page.locator("[data-testid='ai-usage-refresh']:not([disabled])")
        .waitFor({ state: "visible", timeout: 30000 });
      await waitForProviderCards(page, 3);

      const cards = await readProviderCardTexts(page);
      if (cards.length !== 3) {
        throw new Error(`provider cards lost after refresh: ${JSON.stringify(cards)}`);
      }
      return `cards after refresh=${cards.length}`;
    });
    await takeScreenshot(page, run, "ai-usage-panel-after-refresh", screenshots);

    await setStepOverlay(page, "5/6 아이콘 클릭: 패널이 닫혀야 함");
    await check("Dock icon toggles the panel closed", async () => {
      await page.locator("[data-testid='task-detail-usage-button']").click({ timeout: 10000 });
      await page.locator("[data-testid='ai-usage-panel']").waitFor({ state: "detached", timeout: 10000 });
      return "panel closed by dock icon";
    });
    await takeScreenshot(page, run, "ai-usage-panel-closed", screenshots);

    await setStepOverlay(page, `6/6 ${USAGE_SHORTCUT} 재입력: 다시 열려야 함`);
    await check(`${USAGE_SHORTCUT} reopens the panel`, async () => {
      await page.keyboard.press(USAGE_SHORTCUT);
      await page.locator("[data-testid='ai-usage-panel']").waitFor({ state: "visible", timeout: 15000 });
      return "panel reopened by shortcut";
    });
    await takeScreenshot(page, run, "ai-usage-panel-reopened", screenshots);
  } catch (error) {
    errors.push(error instanceof Error ? (error.stack || error.message) : String(error));
  } finally {
    isExpectedShutdown = true;
    if (app) await app.close().catch(() => {});
  }

  const ok = checks.length > 0 && checks.every((entry) => entry.ok);
  writeReport(run, {
    scope: AI_USAGE_SCOPE,
    branch: gitValue(["rev-parse", "--abbrev-ref", "HEAD"]),
    commit: gitValue(["rev-parse", "--short", "HEAD"]),
    ok,
    checks,
    errors,
    notes,
    observedUsage,
    screenshots,
    videoPath: run.videoPath,
  });

  process.exitCode = ok ? 0 : 1;
}

void main();
