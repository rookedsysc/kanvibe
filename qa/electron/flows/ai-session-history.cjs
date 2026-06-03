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
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, `${JSON.stringify(data, null, 2)}\n`, "utf8");
}

function writeJsonLines(filePath, values) {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, `${values.map((value) => JSON.stringify(value)).join("\n")}\n`, "utf8");
}

function renderMarkdown(result) {
  const lines = [];
  lines.push("# KanVibe AI Session History Electron QA Report");
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
}

async function dismissUpdateDialogIfPresent(page) {
  try {
    const closedByDomClick = await page.evaluate(() => {
      const button = document.querySelector("button[aria-label*='업데이트'][aria-label*='닫기']");
      if (!(button instanceof HTMLButtonElement)) return false;
      button.click();
      return true;
    });
    if (closedByDomClick) {
      await page.locator("[data-terminal-focus-blocker][data-shortcut-capture]").waitFor({ state: "detached", timeout: 3000 }).catch(() => {});
      await page.waitForTimeout(300);
      return;
    }
  } catch {
    // Continue with Playwright/user-event fallbacks below.
  }

  const dismissTargets = [
    page.locator("button[aria-label*='업데이트'][aria-label*='닫기']").first(),
    page.getByRole("button", { name: /업데이트.*닫기|닫기|close/i }).first(),
    page.locator("[role='dialog'] button").filter({ hasText: /닫기|close/i }).first(),
  ];

  for (const target of dismissTargets) {
    try {
      if (await target.isVisible({ timeout: 750 })) {
        await target.click({ timeout: 2500, force: true });
        await page.locator("[data-terminal-focus-blocker][data-shortcut-capture]").waitFor({ state: "detached", timeout: 5000 }).catch(() => {});
        await page.waitForTimeout(300);
        return;
      }
    } catch {
      // Optional release/update dialog may not be present or may already be gone.
    }
  }

  try {
    await page.keyboard.press("Escape");
    await page.locator("[data-terminal-focus-blocker][data-shortcut-capture]").waitFor({ state: "detached", timeout: 1500 }).catch(() => {});
  } catch {
    // Optional release/update dialog may not be present.
  }

  try {
    await page.evaluate(() => {
      for (const overlay of document.querySelectorAll("[data-terminal-focus-blocker][data-shortcut-capture]")) {
        if (overlay.querySelector("button[aria-label*='업데이트'][aria-label*='닫기']")) {
          overlay.remove();
        }
      }
    });
  } catch {
    // Last-resort QA cleanup failed; caller assertions will surface any remaining blocker.
  }
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
      el.style.maxWidth = "1080px";
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

function createFixtureRepository(run) {
  const repoDir = path.join(run.runDir, "fixtures", "ai-session-repo");
  const worktreePath = path.join(run.runDir, "fixtures", "ai-session-repo__worktrees", "manual-ai-history");
  fs.rmSync(path.dirname(repoDir), { recursive: true, force: true });
  fs.mkdirSync(repoDir, { recursive: true });
  execFileSync("git", ["init", "-b", "main"], { cwd: repoDir, stdio: "pipe" });
  execFileSync("git", ["config", "user.email", "qa@kanvibe.local"], { cwd: repoDir, stdio: "pipe" });
  execFileSync("git", ["config", "user.name", "KanVibe QA"], { cwd: repoDir, stdio: "pipe" });
  fs.writeFileSync(path.join(repoDir, "README.md"), "# KanVibe AI session QA fixture\n", "utf8");
  execFileSync("git", ["add", "README.md"], { cwd: repoDir, stdio: "pipe" });
  execFileSync("git", ["commit", "-m", "Initial fixture commit"], { cwd: repoDir, stdio: "pipe" });
  execFileSync("git", ["worktree", "add", "-b", "feat/manual-ai-history", worktreePath, "main"], { cwd: repoDir, stdio: "pipe" });
  return { repoDir, worktreePath, branchName: "feat/manual-ai-history" };
}

function claudeProjectDirectoryName(targetPath) {
  return path.resolve(targetPath).replaceAll(path.sep, "-").replaceAll("_", "-");
}

function createFakeAiSessionHistory(fakeHome, fixture) {
  // This QA task is intentionally sessionless, so the app reads AI history for task.project.repoPath.
  // Use repoDir as the matched path to exercise the same manual-load/provider/detail/filter UI path without spawning tmux/zellij.
  const matchedPath = fixture.repoDir;
  const now = "2026-01-01T00:00:00.000Z";

  const claudeFile = path.join(fakeHome, ".claude", "projects", claudeProjectDirectoryName(matchedPath), "claude-manual-ai-history.jsonl");
  writeJsonLines(claudeFile, [
    { type: "system", sessionId: "claude-manual-ai-history", cwd: matchedPath, timestamp: "2026-01-01T00:00:00.000Z", message: { role: "system", content: "Claude system instructions for KanVibe QA." } },
    { type: "user", sessionId: "claude-manual-ai-history", cwd: matchedPath, timestamp: "2026-01-01T00:01:00.000Z", message: { role: "user", content: [{ type: "text", text: "Claude QA prompt: verify manual AI history loading" }] } },
    { type: "assistant", sessionId: "claude-manual-ai-history", cwd: matchedPath, timestamp: "2026-01-01T00:02:00.000Z", message: { role: "assistant", content: [{ type: "text", text: "Claude QA assistant answer: load only after the user presses the history button. Body-search marker: database migration rollback." }] } },
    { type: "user", sessionId: "claude-manual-ai-history", cwd: matchedPath, timestamp: "2026-01-01T00:03:00.000Z", message: { role: "user", content: [{ type: "tool_result", content: "Claude QA tool result text." }] } },
  ]);

  const codexFile = path.join(fakeHome, ".codex", "sessions", "2026", "codex-manual-ai-history.jsonl");
  writeJsonLines(codexFile, [
    { type: "session_meta", timestamp: now, payload: { id: "codex-manual-ai-history", cwd: matchedPath, timestamp: now } },
    { type: "response_item", timestamp: "2026-01-01T00:04:00.000Z", payload: { type: "message", role: "developer", content: [{ type: "input_text", text: "Codex QA developer instruction." }] } },
    { type: "response_item", timestamp: "2026-01-01T00:05:00.000Z", payload: { type: "message", role: "user", content: [{ type: "input_text", text: "Codex QA prompt: confirm unified provider list" }] } },
    { type: "response_item", timestamp: "2026-01-01T00:06:00.000Z", payload: { type: "reasoning", text: "Codex QA reasoning: inspect provider filters." } },
    { type: "response_item", timestamp: "2026-01-01T00:07:00.000Z", payload: { type: "function_call", name: "read_file", arguments: "{\"path\":\"TaskDetailRoute.tsx\"}" } },
    { type: "response_item", timestamp: "2026-01-01T00:08:00.000Z", payload: { type: "function_call_output", output: "TaskDetailRoute contains InlineAiChatView." } },
    { type: "response_item", timestamp: "2026-01-01T00:09:00.000Z", payload: { type: "message", role: "assistant", content: [{ type: "output_text", text: "Codex QA assistant answer: all providers are shown." }] } },
  ]);

  const geminiProjectId = "manual-ai-history-project";
  const geminiChatFile = path.join(fakeHome, ".gemini", "tmp", geminiProjectId, "chats", "session-2026-01-01T00-10-gemini.json");
  writeJson(path.join(fakeHome, ".gemini", "projects.json"), {
    projects: {
      [path.resolve(matchedPath)]: geminiProjectId,
    },
  });
  writeJson(geminiChatFile, {
    sessionId: "gemini-manual-ai-history",
    title: "Gemini QA answer",
    projectHash: geminiProjectId,
    startTime: "2026-01-01T00:10:00.000Z",
    lastUpdated: "2026-01-01T00:14:00.000Z",
    messages: [
      { id: "gemini-user-1", timestamp: "2026-01-01T00:11:00.000Z", type: "user", content: [{ text: "Gemini QA prompt: show role filter buttons" }] },
      {
        id: "gemini-answer-1",
        timestamp: "2026-01-01T00:12:00.000Z",
        type: "gemini",
        content: [{ text: "Gemini QA assistant answer: role filters are visible." }],
        thoughts: [{ subject: "analysis", description: "Gemini QA reasoning text." }],
        toolCalls: [{ name: "read_file", args: { path: "messages/ko.json" }, result: "role labels exist" }],
      },
      { id: "gemini-info-1", timestamp: "2026-01-01T00:13:00.000Z", type: "info", content: "Gemini QA system checkpoint." },
    ],
  });

  const dbPath = path.join(fakeHome, ".local", "share", "opencode", "opencode.db");
  fs.mkdirSync(path.dirname(dbPath), { recursive: true });
  const sqliteFixtureScript = `
import json
import sqlite3
import sys

db_path, matched_path = sys.argv[1], sys.argv[2]
connection = sqlite3.connect(db_path)
cursor = connection.cursor()
cursor.executescript("""
  CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT NOT NULL, title TEXT, time_created INTEGER, time_updated INTEGER);
  CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, data TEXT NOT NULL);
  CREATE TABLE part (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, message_id TEXT NOT NULL, data TEXT NOT NULL, time_created INTEGER NOT NULL);
""")
cursor.execute(
  "INSERT INTO session (id, directory, title, time_created, time_updated) VALUES (?, ?, ?, ?, ?)",
  ("opencode-manual-ai-history", matched_path, "OpenCode QA answer", 1767226500000, 1767226740000),
)
rows = [
  ("opencode-message-user", "user", "text", "OpenCode QA prompt: validate provider badge", 1767226560000),
  ("opencode-message-assistant", "assistant", "text", "OpenCode QA assistant answer: provider badge is visible.", 1767226620000),
  ("opencode-message-reasoning", "assistant", "reasoning", "OpenCode QA reasoning text.", 1767226680000),
  ("opencode-message-tool", "assistant", "tool", {"tool": "read_file", "state": {"output": "OpenCode QA tool output."}}, 1767226740000),
]
for message_id, role, part_type, content, timestamp in rows:
  cursor.execute(
    "INSERT INTO message (id, session_id, data) VALUES (?, ?, ?)",
    (message_id, "opencode-manual-ai-history", json.dumps({"role": role})),
  )
  if part_type == "tool":
    data = {"type": "tool", **content}
  else:
    data = {"type": part_type, "text": content}
  cursor.execute(
    "INSERT INTO part (id, session_id, message_id, data, time_created) VALUES (?, ?, ?, ?, ?)",
    (f"{message_id}-part", "opencode-manual-ai-history", message_id, json.dumps(data), timestamp),
  )
connection.commit()
connection.close()
`;
  execFileSync("python3", ["-c", sqliteFixtureScript, dbPath, matchedPath], { stdio: "pipe" });

  return { claudeFile, codexFile, geminiChatFile, openCodeDbPath: dbPath };
}

async function waitForText(page, text) {
  await page.getByText(text, { exact: false }).first().waitFor({ state: "visible", timeout: 15000 });
}

async function assertProviderBadges(page) {
  for (const provider of ["gemini", "claude", "opencode", "codex"]) {
    await page.locator(`[data-testid='ai-session-provider-${provider}']`).first().waitFor({ state: "visible", timeout: 15000 });
  }
}

async function getSessionListText(page) {
  return page.locator("[data-testid='ai-session-list']").evaluate((node) => node.textContent || "");
}

function assertTextVisibility(text, expectedPresent, expectedAbsent) {
  for (const value of expectedPresent) {
    if (!text.includes(value)) throw new Error(`expected list/detail text to include ${value}; got ${text}`);
  }
  for (const value of expectedAbsent) {
    if (text.includes(value)) throw new Error(`expected list/detail text to exclude ${value}; got ${text}`);
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

  const checks = [];
  const screenshots = [];
  const errors = [];
  const diagnostics = [];
  const notes = [];
  const cdpCollector = createCdpDiagnosticsCollector();
  const fakeHome = path.join(run.runDir, "fake-home");
  const previousHome = process.env.HOME;
  let app;
  let page;
  let cdpSession;
  let traceStarted = false;
  let fixture;
  let seededTask;

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

  const optionalCheck = async (name, fn) => {
    try {
      const detail = await fn();
      checks.push({ name, ok: true, detail: detail || "ok" });
      return detail;
    } catch (error) {
      const detail = error instanceof Error ? (error.stack || error.message) : String(error);
      checks.push({ name, ok: false, detail });
      return null;
    }
  };

  try {
    fixture = createFixtureRepository(run);
    const aiFixtures = createFakeAiSessionHistory(fakeHome, fixture);
    process.env.HOME = fakeHome;

    const launched = await launchKanVibeElectron({
      rootDir: process.cwd(),
      outputDir: run.runDir,
      appDataDir: path.join(run.runDir, "app-data"),
      viewport: { width: 1600, height: 1000 },
    });
    app = launched.app;
    page = launched.page;

    page.on("console", (message) => {
      if (["error", "warning"].includes(message.type())) errors.push(`${message.type()}: ${message.text()}`);
    });
    page.on("pageerror", (error) => errors.push(`pageerror: ${error.message}`));
    page.on("requestfailed", (request) => {
      const failure = request.failure();
      if (failure && !/ERR_ABORTED|favicon/i.test(failure.errorText)) errors.push(`requestfailed: ${request.url()} ${failure.errorText}`);
    });

    cdpSession = await attachCdpDiagnostics(page, cdpCollector);
    await page.context().tracing.start({ screenshots: true, snapshots: true, sources: true });
    traceStarted = true;

    await page.waitForLoadState("domcontentloaded");
    await dismissUpdateDialogIfPresent(page);

    await check("Seed isolated git project, fake HOME AI history, and KanVibe task", async () => {
      const projectResult = await invokeDesktop(page, "project", "registerProject", "KanVibe AI History QA", fixture.repoDir);
      if (!projectResult.success || !projectResult.project) {
        throw new Error(projectResult.error || "project registration failed");
      }
      seededTask = await invokeDesktop(page, "kanban", "createTask", {
        title: "QA manual AI session history",
        description: "Electron QA: manual load and role filter for AI session history.",
        branchName: fixture.branchName,
        baseBranch: "main",
        projectId: projectResult.project.id,
      });
      if (!seededTask?.id) throw new Error("task creation failed");
      const task = await invokeDesktop(page, "kanban", "getTaskById", seededTask.id);
      if (!task) throw new Error(`created task not found: ${seededTask.id}`);
      if (task.projectId !== projectResult.project.id) {
        throw new Error(`created task project mismatch: ${task.projectId}`);
      }
      return `task=${seededTask.id}; repo=${fixture.repoDir}; worktree=${fixture.worktreePath}; fakeHome=${fakeHome}; fixtures=${Object.values(aiFixtures).join(",")}`;
    });

    await setStepOverlay(page, "1/5 상세 화면 진입: AI 히스토리는 아직 자동 로드되지 않아야 함");
    await page.evaluate((taskId) => {
      window.location.hash = `#/ko/task/${taskId}`;
    }, seededTask.id);
    await page.waitForTimeout(1200);
    await dismissUpdateDialogIfPresent(page);
    await page.getByText("QA manual AI session history", { exact: false }).first().waitFor({ state: "visible", timeout: 20000 });
    await takeScreenshot(page, run, "task-detail-before-ai-history-load", screenshots);

    await check("Opening AI chat view does not auto-load history", async () => {
      await dismissUpdateDialogIfPresent(page);
      await page.getByRole("button", { name: "AI 채팅" }).click({ timeout: 15000 });
      await page.locator("[data-testid='inline-ai-chat']").waitFor({ state: "visible", timeout: 15000 });
      await waitForText(page, "AI 세션 히스토리는 필요할 때만 불러옵니다.");
      const listCount = await page.locator("[data-testid='ai-session-list']").count();
      if (listCount !== 0) throw new Error(`history list appeared before manual load: ${listCount}`);
      return "inline chat opened with manual-load hint and no session list";
    });
    await takeScreenshot(page, run, "inline-chat-manual-load-hint", screenshots);

    await setStepOverlay(page, "2/6 히스토리 불러오기: Claude/Codex/OpenCode/Gemini provider 목록 표시");
    await check("Manual history load shows Claude, Codex, OpenCode, and Gemini sessions", async () => {
      await page.getByRole("button", { name: "히스토리 불러오기" }).click({ timeout: 15000 });
      await page.locator("[data-testid='ai-session-list']").waitFor({ state: "visible", timeout: 20000 });
      await assertProviderBadges(page);
      for (const text of [
        "Gemini QA answer",
        "Claude QA prompt: verify manual AI history loading",
        "OpenCode QA answer",
        "Codex QA prompt: confirm unified provider list",
      ]) {
        await page.getByText(text, { exact: false }).first().waitFor({ state: "visible", timeout: 15000 });
      }
      return "all four provider icons and expected session titles/prompts are visible";
    });
    await takeScreenshot(page, run, "provider-session-list-visible", screenshots);

    await setStepOverlay(page, "3/6 왼쪽 provider 아이콘 rail: Claude+Gemini OR 필터 동작 검증");
    await check("Provider icon rail filters sessions with OR semantics", async () => {
      for (const provider of ["claude", "opencode", "gemini", "codex"]) {
        await page.locator(`[data-testid='ai-session-filter-${provider}']`).waitFor({ state: "visible", timeout: 10000 });
      }

      await page.locator("[data-testid='ai-session-filter-claude']").click({ timeout: 15000 });
      await page.waitForTimeout(300);
      assertTextVisibility(
        await getSessionListText(page),
        ["Claude QA prompt: verify manual AI history loading"],
        ["Gemini QA answer", "OpenCode QA answer", "Codex QA prompt: confirm unified provider list"],
      );

      await page.locator("[data-testid='ai-session-filter-gemini']").click({ timeout: 15000 });
      await page.waitForTimeout(300);
      assertTextVisibility(
        await getSessionListText(page),
        ["Claude QA prompt: verify manual AI history loading", "Gemini QA answer"],
        ["OpenCode QA answer", "Codex QA prompt: confirm unified provider list"],
      );

      await page.locator("[data-testid='ai-session-filter-claude']").click({ timeout: 15000 });
      await page.waitForTimeout(300);
      assertTextVisibility(
        await getSessionListText(page),
        ["Gemini QA answer"],
        ["Claude QA prompt: verify manual AI history loading", "OpenCode QA answer", "Codex QA prompt: confirm unified provider list"],
      );

      await page.locator("[data-testid='ai-session-filter-claude']").click({ timeout: 15000 });
      await page.waitForTimeout(300);
      assertTextVisibility(
        await getSessionListText(page),
        ["Claude QA prompt: verify manual AI history loading", "Gemini QA answer"],
        ["OpenCode QA answer", "Codex QA prompt: confirm unified provider list"],
      );
      return "Claude+Gemini can be selected together, and deselecting one provider leaves the other provider only";
    });
    await takeScreenshot(page, run, "provider-or-filter-rail", screenshots);

    await setStepOverlay(page, "4/6 Claude 세션 선택: 말풍선 왼쪽 위 provider 아이콘과 user/assistant/system/tool 메시지 표시");
    await check("Selecting a Claude session loads chat messages with provider icons", async () => {
      await page.getByRole("button", { name: /claude .*manual AI history|claude .*Claude QA prompt/i }).click({ timeout: 15000 });
      await waitForText(page, "Claude QA assistant answer");
      await waitForText(page, "Claude system instructions");
      await waitForText(page, "Claude QA tool result text");
      const messageProviderIconCount = await page.locator("[data-testid='inline-ai-chat'] [data-testid='ai-session-provider-claude']").count();
      if (messageProviderIconCount < 1) throw new Error("Claude provider icon was not rendered in the selected chat UI");
      return "Claude detail messages loaded from fake HOME JSONL with provider icons rendered in the chat UI";
    });
    await takeScreenshot(page, run, "claude-session-detail-messages", screenshots);

    await setStepOverlay(page, "5/6 역할 필터: 시스템 입력만 선택하면 system 메시지만 남아야 함");
    await check("Role filter passes through and narrows detail to system messages", async () => {
      await page.getByRole("button", { name: "시스템 입력" }).click({ timeout: 15000 });
      await page.waitForTimeout(1000);
      await waitForText(page, "Claude system instructions");
      const detailText = await page.locator("[data-testid='inline-ai-chat']").evaluate((root) => {
        const body = root.children.item(1);
        const detailColumn = body?.children.item(1);
        const messageArea = detailColumn?.lastElementChild;
        return messageArea?.textContent || "";
      });
      const assistantVisible = detailText.includes("Claude QA assistant answer");
      const userVisible = detailText.includes("Claude QA prompt: verify manual AI history loading");
      const toolVisible = detailText.includes("Claude QA tool result text");
      if (assistantVisible || userVisible || toolVisible) {
        throw new Error(`non-system messages remained visible: assistant=${assistantVisible}, user=${userVisible}, tool=${toolVisible}`);
      }
      return "system role filter left only the system message visible";
    });
    await takeScreenshot(page, run, "system-role-filter-applied", screenshots);

    await setStepOverlay(page, "6/6 채팅 검색: 본문에만 있는 database migration 문구로 Claude 대화 검색");
    await check("Chat search reloads history by message-body query and opens the matching conversation", async () => {
      const searchInput = page.locator("#ai-session-search");
      await searchInput.fill("database migration rollback");
      await page.getByRole("button", { name: "검색" }).click({ timeout: 15000 });
      await page.locator("[data-testid='ai-session-list']").waitFor({ state: "visible", timeout: 20000 });
      assertTextVisibility(
        await getSessionListText(page),
        ["Claude QA prompt: verify manual AI history loading"],
        ["Gemini QA answer", "OpenCode QA answer", "Codex QA prompt: confirm unified provider list"],
      );
      await page.getByRole("button", { name: /claude .*manual AI history|claude .*Claude QA prompt/i }).click({ timeout: 15000 });
      await waitForText(page, "Body-search marker: database migration rollback");
      const detailText = await page.locator("[data-testid='inline-ai-chat']").evaluate((root) => {
        const body = root.children.item(1);
        const detailColumn = body?.children.item(1);
        const messageArea = detailColumn?.lastElementChild;
        return messageArea?.textContent || "";
      });
      assertTextVisibility(detailText, ["Body-search marker: database migration rollback"], ["Claude system instructions", "Claude QA tool result text"]);
      return "search query matched body-only Claude assistant text, narrowed the list, and reopened the matching chat message";
    });
    await takeScreenshot(page, run, "body-chat-search-filtered", screenshots);

    await check("Role filter UI exposes extended roles", async () => {
      for (const label of ["사용자 입력", "AI 답변", "시스템 입력", "개발자 입력", "추론", "도구", "기타"]) {
        await page.getByRole("button", { name: label }).waitFor({ state: "visible", timeout: 10000 });
      }
      return "all role filter buttons are visible";
    });

    await check("CDP runtime/network diagnostics captured", async () => {
      const metrics = cdpSession ? await cdpSession.send("Performance.getMetrics") : { metrics: [] };
      const metricNames = new Set((metrics.metrics || []).map((metric) => metric.name));
      if (!metricNames.has("Timestamp")) throw new Error("CDP Performance metrics did not include Timestamp");
      return `requests=${cdpCollector.counters.requests}, responses=${cdpCollector.counters.responses}, failures=${cdpCollector.counters.failedRequests}, exceptions=${cdpCollector.counters.exceptions}`;
    });

    await check("No blocking console errors", async () => {
      const blocking = errors.filter((line) => !/favicon|DevTools|Electron Security Warning|Insecure Content-Security-Policy/i.test(line));
      if (blocking.length > 0) throw new Error(blocking.join("\n"));
      return "none";
    });
  } catch (error) {
    const detail = error instanceof Error ? (error.stack || error.message) : String(error);
    console.error(`[kanvibe-qa] AI session history flow failed:\n${detail}`);
    checks.push({ name: "AI session history Electron QA flow", ok: false, detail });
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
    process.env.HOME = previousHome;
  }

  await optionalCheck("Playwright trace artifact written", async () => {
    if (!fs.existsSync(run.tracePath) || fs.statSync(run.tracePath).size === 0) {
      throw new Error(`trace missing or empty: ${run.tracePath}`);
    }
    return run.tracePath;
  });

  await optionalCheck("CDP diagnostics artifact written", async () => {
    if (!fs.existsSync(run.cdpDiagnosticsPath) || fs.statSync(run.cdpDiagnosticsPath).size === 0) {
      throw new Error(`CDP diagnostics missing or empty: ${run.cdpDiagnosticsPath}`);
    }
    return run.cdpDiagnosticsPath;
  });

  const ok = checks.every((item) => item.ok);
  if (fs.existsSync(run.videoPath)) notes.push("Actual X11 screen recording captured with ffmpeg.");
  else notes.push("No mp4 was present when flow finished; wrapper script is expected to fail if recording is missing.");
  notes.push(`Fake HOME for provider history fixtures: ${fakeHome}`);
  if (fixture) notes.push(`Fixture repo/worktree: ${fixture.repoDir} / ${fixture.worktreePath}`);
  notes.push("QA uses isolated KANVIBE_APP_DATA_DIR and fake HOME under the run directory, not the user's app data or real AI history.");
  diagnostics.push(`CDP counters: ${JSON.stringify(cdpCollector.counters)}`);
  diagnostics.push("Playwright trace captures actions, screenshots, DOM snapshots, console, and network timeline.");

  const result = {
    ok,
    scope: "Electron UI QA for AI session history controls: manual history loading, provider icon rail OR filtering, message-level provider icons, body-text chat search across Claude/Codex/OpenCode/Gemini history, session detail rendering, and role filter narrowing",
    branch: gitValue(["branch", "--show-current"]),
    commit: gitValue(["rev-parse", "--short", "HEAD"]),
    checks,
    errors,
    diagnostics,
    screenshots,
    videoPath: fs.existsSync(run.videoPath) ? run.videoPath : null,
    tracePath: fs.existsSync(run.tracePath) ? run.tracePath : null,
    cdpDiagnosticsPath: fs.existsSync(run.cdpDiagnosticsPath) ? run.cdpDiagnosticsPath : null,
    notes,
  };

  writeReport(run, result);
  console.log(JSON.stringify({ ok, runDir: run.runDir, reportPath: path.join(run.runDir, "report.md"), videoPath: run.videoPath }, null, 2));
  process.exit(ok ? 0 : 1);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
