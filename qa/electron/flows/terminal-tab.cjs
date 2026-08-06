#!/usr/bin/env node
/* eslint-disable @typescript-eslint/no-require-imports */
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const { createQaRun } = require("../lib/report.cjs");
const { launchKanVibeElectron } = require("../lib/launchElectron.cjs");

const TERMINAL_TAB_SCOPE = "터미널 탭: terminal 세션에서 탭 생성·전환·이름변경·드래그·닫기와 단축키를 검증하고, tmux window와 zellij tab 각각에 대해 KanVibe 밖의 변경이 탭 바에 반영되는지와 그 반대 방향을 확인한다";

/** Linux Electron에서 Mod는 Control이다 */
const MOD_KEY = process.platform === "darwin" ? "Meta" : "Control";

/** 정리 명령이 응답하지 않아도 QA 실행이 끝나야 한다 */
const SESSION_CLEANUP_TIMEOUT_MS = 10_000;

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

/** tmux 동기화 검증은 KanVibe 밖에서 같은 세션을 직접 조작해야 성립한다 */
function execTmux(args) {
  return execFileSync("tmux", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

/** zellij도 같은 이유로 KanVibe 밖에서 직접 조작한다 */
function execZellij(sessionName, actionArgs) {
  return execFileSync("zellij", ["--session", sessionName, "action", ...actionArgs], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

function readZellijTabs(sessionName) {
  return JSON.parse(execZellij(sessionName, ["list-tabs", "--json"]))
    .map((tab) => ({ id: String(tab.tab_id), index: tab.position, name: tab.name, isActive: tab.active }));
}

function writeJson(filePath, data) {
  fs.writeFileSync(filePath, `${JSON.stringify(data, null, 2)}\n`, "utf8");
}

function ensureDir(dirPath) {
  fs.mkdirSync(dirPath, { recursive: true });
}

function renderReport(result) {
  const lines = [];
  lines.push("# KanVibe Terminal Tab Electron QA Report");
  lines.push("");
  lines.push(`- Run ID: \`${result.runId}\``);
  lines.push(`- Branch: \`${result.branch || "unknown"}\``);
  lines.push(`- Commit: \`${result.commit || "unknown"}\``);
  lines.push(`- Scope: ${result.scope}`);
  lines.push(`- Status: **${result.ok ? "PASS" : "FAIL"}**`);
  lines.push("");
  lines.push("## Checks");
  lines.push("");
  for (const check of result.checks) {
    lines.push(`- ${check.ok ? "✅" : "❌"} **${check.name}**${check.detail ? ` — ${check.detail}` : ""}`);
  }
  lines.push("");
  lines.push("## Console / Runtime Errors");
  lines.push("");
  if (result.errors.length > 0) {
    for (const error of result.errors) lines.push(`- ${error}`);
  } else {
    lines.push("No blocking console/runtime errors captured.");
  }
  lines.push("");
  lines.push("## Evidence");
  lines.push("");
  for (const shot of result.screenshots) {
    lines.push(`- ${shot.label}: \`${shot.path}\``);
  }
  if (result.videoPath) lines.push(`- Video: \`${result.videoPath}\``);
  if (result.tracePath) lines.push(`- Playwright trace: \`${result.tracePath}\``);
  lines.push("");
  lines.push("## Notes");
  lines.push("");
  for (const note of result.notes) lines.push(`- ${note}`);
  lines.push("");
  return `${lines.join("\n")}\n`;
}

function writeReport(run, result) {
  const fullResult = { ...result, runId: run.runId };
  writeJson(run.jsonPath, fullResult);
  fs.writeFileSync(run.reportPath, renderReport(fullResult), "utf8");
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

async function takeScreenshot(page, run, label, screenshots) {
  const fileName = `${String(screenshots.length + 1).padStart(2, "0")}-${label}.png`;
  const shotPath = path.join(run.screenshotsDir, fileName);
  await page.screenshot({ path: shotPath, fullPage: true });
  screenshots.push({ label, path: shotPath });
}

async function invokeDesktop(page, namespace, method, ...args) {
  return page.evaluate(
    async ({ namespace: serviceNamespace, method: serviceMethod, args: serviceArgs }) => (
      window.kanvibeDesktop.invoke(serviceNamespace, serviceMethod, serviceArgs)
    ),
    { namespace, method, args },
  );
}

async function dismissUpdateDialogIfPresent(page) {
  const closeButton = page.getByRole("button", { name: /닫기|Close/i }).first();
  try {
    await closeButton.click({ timeout: 2500 });
    await page.waitForTimeout(500);
  } catch {
    // Optional release/update dialog is not always present.
  }
}

function createLocalFixtureRepository(run) {
  const projectDir = path.join(run.runDir, "fixtures", `terminal-tab-${run.runId}`);
  const projectName = `Terminal Tab QA ${run.runId}`;
  const defaultBranch = "main";
  fs.rmSync(projectDir, { recursive: true, force: true });
  ensureDir(projectDir);

  try {
    execGit(["init", "--initial-branch", defaultBranch], projectDir);
  } catch {
    execGit(["init"], projectDir);
    execGit(["checkout", "-B", defaultBranch], projectDir);
  }
  execGit(["config", "user.email", "qa@kanvibe.local"], projectDir);
  execGit(["config", "user.name", "KanVibe QA"], projectDir);
  fs.writeFileSync(path.join(projectDir, "README.md"), `# ${projectName}\n`, "utf8");
  execGit(["add", "README.md"], projectDir);
  execGit(["commit", "-m", "Initial QA fixture"], projectDir);

  return { projectDir, projectName, defaultBranch };
}

/**
 * 기본으로 열리는 작업 정보 패널은 터미널 크롬 위를 덮어 탭 바 클릭을 가로챈다.
 * 앱이 설계한 해제 방법대로 패널 밖 터미널 본문을 눌러 닫는다.
 */
async function dismissDetailPanel(page) {
  const viewport = page.viewportSize() || { width: 1500, height: 950 };
  await page.mouse.click(Math.round(viewport.width * 0.75), Math.round(viewport.height * 0.6));
  await page.waitForTimeout(500);
}

async function openTaskDetail(page, taskId) {
  await page.evaluate((id) => {
    window.location.hash = `#/ko/task/${id}`;
  }, taskId);
  await page.getByTestId("terminal-tab-bar").waitFor({ state: "visible", timeout: 20000 });
  await dismissDetailPanel(page);
  /** 첫 탭의 PTY가 붙고 폴링이 한 바퀴 돌 시간을 준다 */
  await page.waitForTimeout(1500);
}

async function readTabs(page) {
  return page.evaluate(() => (
    [...document.querySelectorAll("[data-terminal-tab-id]")].map((element) => ({
      id: element.getAttribute("data-terminal-tab-id"),
      /** 탭 안에는 이름 말고 단축키 힌트와 닫기 버튼도 있으므로 이름 노드만 읽는다 */
      name: (element.querySelector("[data-terminal-tab-name]") ?? element).textContent.trim(),
      isActive: element.getAttribute("aria-selected") === "true",
    }))
  ));
}

/** 폴링이나 낙관적 갱신이 반영될 때까지 기다린다 */
async function waitForTabs(page, predicate, description, timeoutMs = 15000) {
  const deadline = Date.now() + timeoutMs;
  let lastTabs = [];

  while (Date.now() < deadline) {
    lastTabs = await readTabs(page);
    if (predicate(lastTabs)) return lastTabs;
    await page.waitForTimeout(300);
  }

  throw new Error(`${description}: ${JSON.stringify(lastTabs)}`);
}

async function connectSession(page, taskId, sessionType) {
  const task = await invokeDesktop(page, "kanban", "connectTerminalSession", taskId, sessionType);
  if (!task || task.sessionType !== sessionType) {
    throw new Error(`connectTerminalSession(${sessionType}) failed: ${JSON.stringify(task)}`);
  }
  return task;
}

async function seedTerminalTabData(page, fixture, runId) {
  const projectResult = await invokeDesktop(page, "project", "registerProject", fixture.projectName, fixture.projectDir);
  if (!projectResult.success || !projectResult.project) {
    throw new Error(projectResult.error || "QA project registration failed");
  }

  const project = projectResult.project;
  const rootTaskId = await invokeDesktop(page, "kanban", "getTaskIdByProjectAndBranch", project.id, fixture.defaultBranch);
  if (!rootTaskId) {
    throw new Error("Project root task was not created for the fixture project");
  }

  const tmuxTask = await invokeDesktop(page, "kanban", "createTask", {
    title: `qa-terminal-tab-tmux-${runId}`,
    description: "Electron QA task for tmux window ↔ KanVibe tab sync.",
    projectId: project.id,
    baseBranch: fixture.defaultBranch,
  });
  const zellijTask = await invokeDesktop(page, "kanban", "createTask", {
    title: `qa-terminal-tab-zellij-${runId}`,
    description: "Electron QA task for zellij tab ↔ KanVibe tab sync.",
    projectId: project.id,
    baseBranch: fixture.defaultBranch,
  });

  return { project, rootTaskId, tmuxTaskId: tmuxTask.id, zellijTaskId: zellijTask.id };
}

async function main() {
  const options = parseArgs(process.argv);
  const run = createQaRun({ ...options, rootDir: process.cwd() });
  const fixture = createLocalFixtureRepository(run);
  const checks = [];
  const screenshots = [];
  const consoleErrors = [];
  const notes = [];
  let app = null;
  let page = null;
  let traceStarted = false;
  let tmuxSessionName = null;
  let zellijSessionName = null;

  try {
    const launched = await launchKanVibeElectron({
      rootDir: run.rootDir,
      outputDir: run.runDir,
      appDataDir: path.join(run.runDir, "app-data"),
      viewport: { width: 1500, height: 950 },
      actionTimeoutMs: 15000,
    });
    app = launched.app;
    page = launched.page;

    page.on("console", (message) => {
      if (["error", "warning"].includes(message.type())) {
        consoleErrors.push(`${message.type()}: ${message.text()}`);
      }
    });
    page.on("pageerror", (error) => consoleErrors.push(`pageerror: ${error.message}`));

    await page.context().tracing.start({ screenshots: true, snapshots: true, sources: true });
    traceStarted = true;

    await optionalStep(checks, "Electron 창이 KanVibe 보드로 열린다", async () => {
      await page.waitForLoadState("domcontentloaded");
      await page.locator("body").waitFor({ state: "visible", timeout: 20000 });
      await dismissUpdateDialogIfPresent(page);
      return page.url();
    });

    let seeded = null;
    await optionalStep(checks, "QA 전용 프로젝트와 태스크를 앱 IPC로 준비한다", async () => {
      seeded = await seedTerminalTabData(page, fixture, run.runId);
      return `project=${seeded.project.name}, root=${seeded.rootTaskId}, tmux=${seeded.tmuxTaskId}`;
    });
    if (!seeded) throw new Error("QA seed 실패로 터미널 탭 플로우를 진행할 수 없다");

    // --- 요구사항 ②③: 멀티플렉서 없는 terminal 세션 ---

    await optionalStep(checks, "요구사항③ terminal 세션 타입이 저장되고 그대로 읽힌다", async () => {
      const task = await connectSession(page, seeded.rootTaskId, "terminal");
      const reloaded = await invokeDesktop(page, "kanban", "getTaskById", seeded.rootTaskId);
      if (reloaded.sessionType !== "terminal") {
        throw new Error(`저장된 sessionType이 되돌아오지 않는다: ${reloaded.sessionType}`);
      }
      return `sessionType=${task.sessionType}, sessionName=${task.sessionName}`;
    });

    await optionalStep(checks, "요구사항② tmux/zellij 없이 터미널이 열리고 탭 바에 탭 하나가 보인다", async () => {
      await openTaskDetail(page, seeded.rootTaskId);
      const tabs = await waitForTabs(page, (list) => list.length === 1 && list[0].isActive, "탭 하나가 활성으로 보이지 않는다");
      await takeScreenshot(page, run, "terminal-session-single-tab", screenshots);
      return `tabs=${JSON.stringify(tabs)}`;
    });

    await optionalStep(checks, "기본으로 열리는 작업 정보 패널이 탭 바를 가리지 않는다", async () => {
      /** dock 단축키로 작업 정보 패널을 다시 띄운 뒤 실제 좌표와 클릭 도달 여부를 함께 본다 */
      await page.keyboard.press(process.platform === "darwin" ? "Meta+1" : "Alt+1");
      await page.getByTestId("task-detail-panel").waitFor({ state: "visible", timeout: 20000 });

      const geometry = await page.evaluate(() => {
        const panel = document.querySelector("[data-testid='task-detail-panel']");
        const tabBar = document.querySelector("[data-testid='terminal-tab-bar']");
        if (!panel || !tabBar) return null;
        const panelRect = panel.getBoundingClientRect();
        const tabBarRect = tabBar.getBoundingClientRect();
        return { panelTop: panelRect.top, tabBarBottom: tabBarRect.bottom };
      });
      if (!geometry) throw new Error("패널이나 탭 바를 찾지 못했다");
      if (geometry.panelTop < geometry.tabBarBottom) {
        throw new Error(`패널이 탭 바를 덮는다: panelTop=${geometry.panelTop}, tabBarBottom=${geometry.tabBarBottom}`);
      }

      await page.getByTestId("terminal-tab-new").click({ timeout: 5000 });
      const tabs = await waitForTabs(page, (list) => list.length >= 2, "패널이 열린 상태에서 + 버튼이 동작하지 않았다");
      await takeScreenshot(page, run, "detail-panel-does-not-cover-tab-bar", screenshots);

      const closableTab = tabs.at(-1);
      await page.locator(`[data-terminal-tab-id='${closableTab.id}']`).hover();
      await page.getByTestId(`terminal-tab-close-${closableTab.id}`).click();
      await waitForTabs(page, (list) => list.length === 1, "검증용 탭을 되돌리지 못했다");
      await dismissDetailPanel(page);

      return `panelTop=${Math.round(geometry.panelTop)} >= tabBarBottom=${Math.round(geometry.tabBarBottom)}, 패널이 열린 채로도 + 버튼 클릭 성공`;
    });

    await optionalStep(checks, "요구사항① + 버튼으로 탭을 추가하면 새 탭이 활성이 된다", async () => {
      await page.getByTestId("terminal-tab-new").click();
      const tabs = await waitForTabs(
        page,
        (list) => list.length === 2 && list[1].isActive,
        "두 번째 탭이 활성으로 추가되지 않았다",
      );
      await takeScreenshot(page, run, "terminal-session-two-tabs", screenshots);
      return `tabs=${JSON.stringify(tabs)}`;
    });

    await optionalStep(checks, "요구사항① 탭을 클릭하면 그 탭으로 전환된다", async () => {
      const [firstTab] = await readTabs(page);
      await page.locator(`[data-terminal-tab-id='${firstTab.id}']`).click();
      const tabs = await waitForTabs(
        page,
        (list) => list[0].id === firstTab.id && list[0].isActive,
        "첫 탭으로 전환되지 않았다",
      );
      await takeScreenshot(page, run, "terminal-session-tab-switched", screenshots);
      return `active=${tabs.find((tab) => tab.isActive).id}`;
    });

    await optionalStep(checks, "요구사항① 더블클릭 후 Enter로 탭 이름을 바꾼다", async () => {
      const [firstTab] = await readTabs(page);
      await page.locator(`[data-terminal-tab-id='${firstTab.id}']`).dblclick();
      const renameInput = page.getByTestId("terminal-tab-rename-input");
      await renameInput.waitFor({ state: "visible", timeout: 10000 });
      await renameInput.fill("빌드");
      await renameInput.press("Enter");
      const tabs = await waitForTabs(
        page,
        (list) => list.some((tab) => tab.id === firstTab.id && tab.name === "빌드"),
        "탭 이름이 바뀌지 않았다",
      );
      await takeScreenshot(page, run, "terminal-session-tab-renamed", screenshots);
      return `tabs=${JSON.stringify(tabs)}`;
    });

    await optionalStep(checks, "요구사항① 드래그로 탭 순서를 바꾼다", async () => {
      const before = await readTabs(page);
      await page.locator(`[data-terminal-tab-id='${before[0].id}']`)
        .dragTo(page.locator(`[data-terminal-tab-id='${before[1].id}']`));
      const tabs = await waitForTabs(
        page,
        (list) => list[0].id === before[1].id && list[1].id === before[0].id,
        "탭 순서가 바뀌지 않았다",
      );
      await takeScreenshot(page, run, "terminal-session-tab-reordered", screenshots);
      return `before=[${before.map((tab) => tab.name)}], after=[${tabs.map((tab) => tab.name)}]`;
    });

    await optionalStep(checks, `요구사항① ${MOD_KEY}+T 단축키가 터미널보다 먼저 잡혀 탭을 만든다`, async () => {
      const before = await readTabs(page);
      await page.keyboard.press(`${MOD_KEY}+t`);
      const tabs = await waitForTabs(
        page,
        (list) => list.length === before.length + 1,
        `${MOD_KEY}+T로 탭이 추가되지 않았다`,
      );
      await takeScreenshot(page, run, "terminal-session-shortcut-new-tab", screenshots);
      return `${before.length} -> ${tabs.length}`;
    });

    await optionalStep(checks, `요구사항① ${MOD_KEY}+Shift+[ 로 이전 탭으로 이동한다`, async () => {
      const before = await readTabs(page);
      const beforeActiveIndex = before.findIndex((tab) => tab.isActive);
      await page.keyboard.press(`${MOD_KEY}+Shift+BracketLeft`);
      const tabs = await waitForTabs(
        page,
        (list) => list.findIndex((tab) => tab.isActive) !== beforeActiveIndex,
        "이전 탭으로 이동하지 않았다",
      );
      await takeScreenshot(page, run, "terminal-session-shortcut-prev-tab", screenshots);
      return `activeIndex ${beforeActiveIndex} -> ${tabs.findIndex((tab) => tab.isActive)}`;
    });

    await optionalStep(checks, "요구사항① × 버튼으로 탭을 닫으면 나머지 탭은 남는다", async () => {
      const before = await readTabs(page);
      const closingTab = before.find((tab) => !tab.isActive) || before[0];
      await page.locator(`[data-terminal-tab-id='${closingTab.id}']`).hover();
      await page.getByTestId(`terminal-tab-close-${closingTab.id}`).click();
      const tabs = await waitForTabs(
        page,
        (list) => list.length === before.length - 1 && !list.some((tab) => tab.id === closingTab.id),
        "탭이 닫히지 않았다",
      );
      await takeScreenshot(page, run, "terminal-session-tab-closed", screenshots);
      return `${before.length} -> ${tabs.length}`;
    });

    // --- 요구사항 ①: tmux window ↔ KanVibe 탭 양방향 동기화 ---

    await optionalStep(checks, "요구사항① tmux 세션 태스크를 열면 tmux window가 탭으로 보인다", async () => {
      const task = await connectSession(page, seeded.tmuxTaskId, "tmux");
      tmuxSessionName = task.sessionName;
      await openTaskDetail(page, seeded.tmuxTaskId);
      const tabs = await waitForTabs(page, (list) => list.length >= 1, "tmux window가 탭으로 보이지 않는다");
      const tmuxWindows = execTmux(["list-windows", "-t", tmuxSessionName, "-F", "#{window_id}"]).split("\n").filter(Boolean);
      if (tabs.length !== tmuxWindows.length) {
        throw new Error(`탭 수와 tmux window 수가 다르다: tabs=${tabs.length}, windows=${tmuxWindows.length}`);
      }
      await takeScreenshot(page, run, "tmux-session-initial-tabs", screenshots);
      return `session=${tmuxSessionName}, windows=${tmuxWindows.join(",")}`;
    });

    await optionalStep(checks, "요구사항① KanVibe 밖에서 만든 tmux window가 폴링으로 탭 바에 나타난다", async () => {
      const before = await readTabs(page);
      execTmux(["new-window", "-t", `${tmuxSessionName}:`, "-n", "outside-kanvibe"]);
      const tabs = await waitForTabs(
        page,
        (list) => list.length === before.length + 1 && list.some((tab) => tab.name.includes("outside-kanvibe")),
        "외부에서 만든 tmux window가 탭 바에 반영되지 않았다",
      );
      await takeScreenshot(page, run, "tmux-external-window-synced", screenshots);
      return `${before.length} -> ${tabs.length}, names=[${tabs.map((tab) => tab.name)}]`;
    });

    await optionalStep(checks, "요구사항① KanVibe 밖에서 바꾼 window 이름도 탭에 반영된다", async () => {
      const externalWindowId = execTmux([
        "list-windows", "-t", tmuxSessionName, "-F", "#{window_id}\t#{window_name}",
      ]).split("\n").find((line) => line.includes("outside-kanvibe")).split("\t")[0];
      execTmux(["rename-window", "-t", externalWindowId, "renamed-outside"]);
      const tabs = await waitForTabs(
        page,
        (list) => list.some((tab) => tab.name.includes("renamed-outside")),
        "외부 이름 변경이 탭에 반영되지 않았다",
      );
      await takeScreenshot(page, run, "tmux-external-rename-synced", screenshots);
      return `names=[${tabs.map((tab) => tab.name)}]`;
    });

    await optionalStep(checks, "요구사항① KanVibe에서 탭을 누르면 tmux의 활성 window가 따라 바뀐다", async () => {
      const tabs = await readTabs(page);
      const targetTab = tabs.find((tab) => !tab.isActive);
      if (!targetTab) throw new Error("전환할 비활성 탭이 없다");

      await page.locator(`[data-terminal-tab-id='${targetTab.id}']`).click();
      await waitForTabs(
        page,
        (list) => list.some((tab) => tab.id === targetTab.id && tab.isActive),
        "탭 바에서 전환이 반영되지 않았다",
      );

      const activeWindowId = execTmux([
        "display-message", "-p", "-t", tmuxSessionName, "#{window_id}",
      ]);
      if (activeWindowId !== targetTab.id) {
        throw new Error(`tmux 활성 window가 따라오지 않았다: tmux=${activeWindowId}, tab=${targetTab.id}`);
      }
      await takeScreenshot(page, run, "tmux-tab-click-selects-window", screenshots);
      return `tmux active window=${activeWindowId} === clicked tab=${targetTab.id}`;
    });

    await optionalStep(checks, "요구사항① KanVibe에서 만든 탭이 tmux window로 생긴다", async () => {
      const beforeWindows = execTmux(["list-windows", "-t", tmuxSessionName, "-F", "#{window_id}"]).split("\n").filter(Boolean);
      await page.getByTestId("terminal-tab-new").click();
      await waitForTabs(page, (list) => list.length === beforeWindows.length + 1, "새 탭이 탭 바에 나타나지 않았다");

      const afterWindows = execTmux(["list-windows", "-t", tmuxSessionName, "-F", "#{window_id}"]).split("\n").filter(Boolean);
      if (afterWindows.length !== beforeWindows.length + 1) {
        throw new Error(`tmux window가 늘지 않았다: ${beforeWindows.length} -> ${afterWindows.length}`);
      }
      await takeScreenshot(page, run, "tmux-new-tab-creates-window", screenshots);
      return `windows ${beforeWindows.length} -> ${afterWindows.length}`;
    });

    await optionalStep(checks, "요구사항① KanVibe에서 닫은 탭의 tmux window도 사라진다", async () => {
      const tabs = await readTabs(page);
      const closingTab = tabs.find((tab) => !tab.isActive) || tabs[0];
      await page.locator(`[data-terminal-tab-id='${closingTab.id}']`).hover();
      await page.getByTestId(`terminal-tab-close-${closingTab.id}`).click();
      await waitForTabs(page, (list) => !list.some((tab) => tab.id === closingTab.id), "탭이 닫히지 않았다");

      const remainingWindows = execTmux(["list-windows", "-t", tmuxSessionName, "-F", "#{window_id}"]).split("\n").filter(Boolean);
      if (remainingWindows.includes(closingTab.id)) {
        throw new Error(`tmux window가 남아 있다: ${closingTab.id}`);
      }
      await takeScreenshot(page, run, "tmux-tab-close-kills-window", screenshots);
      return `remaining windows=${remainingWindows.join(",")}`;
    });

    // --- 요구사항①: zellij tab ↔ KanVibe 탭 양방향 동기화 ---

    await optionalStep(checks, "요구사항① zellij 세션 태스크를 열면 zellij tab이 탭으로 보인다", async () => {
      const task = await connectSession(page, seeded.zellijTaskId, "zellij");
      zellijSessionName = task.sessionName;
      await openTaskDetail(page, seeded.zellijTaskId);
      const tabs = await waitForTabs(page, (list) => list.length >= 1, "zellij tab이 탭으로 보이지 않는다");
      const zellijTabs = readZellijTabs(zellijSessionName);
      if (tabs.length !== zellijTabs.length) {
        throw new Error(`탭 수가 다르다: kanvibe=${tabs.length}, zellij=${zellijTabs.length}`);
      }
      await takeScreenshot(page, run, "zellij-session-initial-tabs", screenshots);
      return `session=${zellijSessionName}, version=${execFileSync("zellij", ["--version"], { encoding: "utf8" }).trim()}, tabs=${zellijTabs.map((tab) => tab.name).join(",")}`;
    });

    await optionalStep(checks, "D7 KanVibe가 만든 zellij 세션은 자체 탭 바·상태바를 띄우지 않는다", async () => {
      const layout = execZellij(zellijSessionName, ["dump-layout"]);
      if (layout.includes("zellij:tab-bar") || layout.includes("zellij:status-bar")) {
        throw new Error("zellij 자체 바 플러그인이 살아 있다");
      }
      return "dump-layout에 tab-bar/status-bar 플러그인 없음";
    });

    await optionalStep(checks, "요구사항① KanVibe 밖에서 만든 zellij tab이 폴링으로 탭 바에 나타난다", async () => {
      const before = await readTabs(page);
      execZellij(zellijSessionName, ["new-tab", "--name", "outside-kanvibe"]);
      const tabs = await waitForTabs(
        page,
        (list) => list.length === before.length + 1 && list.some((tab) => tab.name.includes("outside-kanvibe")),
        "외부에서 만든 zellij tab이 탭 바에 반영되지 않았다",
      );
      await takeScreenshot(page, run, "zellij-external-tab-synced", screenshots);
      return `${before.length} -> ${tabs.length}, names=[${tabs.map((tab) => tab.name)}]`;
    });

    await optionalStep(checks, "요구사항① KanVibe 밖에서 바꾼 zellij tab 이름도 탭에 반영된다", async () => {
      const externalTab = readZellijTabs(zellijSessionName).find((tab) => tab.name.includes("outside-kanvibe"));
      execZellij(zellijSessionName, ["rename-tab-by-id", externalTab.id, "renamed-outside"]);
      const tabs = await waitForTabs(
        page,
        (list) => list.some((tab) => tab.name.includes("renamed-outside")),
        "외부 이름 변경이 탭에 반영되지 않았다",
      );
      await takeScreenshot(page, run, "zellij-external-rename-synced", screenshots);
      return `names=[${tabs.map((tab) => tab.name)}]`;
    });

    await optionalStep(checks, "요구사항① KanVibe에서 탭을 누르면 zellij의 활성 tab이 따라 바뀐다", async () => {
      const tabs = await readTabs(page);
      const targetTab = tabs.find((tab) => !tab.isActive);
      if (!targetTab) throw new Error("전환할 비활성 탭이 없다");

      await page.locator(`[data-terminal-tab-id='${targetTab.id}']`).click();
      await waitForTabs(
        page,
        (list) => list.some((tab) => tab.id === targetTab.id && tab.isActive),
        "탭 바에서 전환이 반영되지 않았다",
      );

      const activeZellijTab = readZellijTabs(zellijSessionName).find((tab) => tab.isActive);
      if (activeZellijTab.id !== targetTab.id) {
        throw new Error(`zellij 활성 tab이 따라오지 않았다: zellij=${activeZellijTab.id}, tab=${targetTab.id}`);
      }
      await takeScreenshot(page, run, "zellij-tab-click-selects-tab", screenshots);
      return `zellij active tab=${activeZellijTab.id} === clicked tab=${targetTab.id}`;
    });

    await optionalStep(checks, "요구사항① KanVibe에서 만든 탭이 zellij tab으로 생긴다", async () => {
      const beforeCount = readZellijTabs(zellijSessionName).length;
      await page.getByTestId("terminal-tab-new").click();
      await waitForTabs(page, (list) => list.length === beforeCount + 1, "새 탭이 탭 바에 나타나지 않았다");

      const afterCount = readZellijTabs(zellijSessionName).length;
      if (afterCount !== beforeCount + 1) {
        throw new Error(`zellij tab이 늘지 않았다: ${beforeCount} -> ${afterCount}`);
      }
      await takeScreenshot(page, run, "zellij-new-tab-creates-tab", screenshots);
      return `zellij tabs ${beforeCount} -> ${afterCount}`;
    });

    await optionalStep(checks, "요구사항① 가운데 탭을 닫아도 tab_id 기준이라 남은 탭이 정확하다", async () => {
      const tabsBefore = await readTabs(page);
      if (tabsBefore.length < 3) throw new Error(`검증에는 탭 3개 이상이 필요하다: ${tabsBefore.length}`);

      /** position이 아니라 tab_id를 식별자로 써야 가운데를 닫았을 때 나머지가 밀리지 않는다 */
      const middleTab = tabsBefore[1];
      const survivingIds = tabsBefore.filter((tab) => tab.id !== middleTab.id).map((tab) => tab.id);
      await page.locator(`[data-terminal-tab-id='${middleTab.id}']`).hover();
      await page.getByTestId(`terminal-tab-close-${middleTab.id}`).click();
      await waitForTabs(page, (list) => !list.some((tab) => tab.id === middleTab.id), "가운데 탭이 닫히지 않았다");

      const remainingZellijIds = readZellijTabs(zellijSessionName).map((tab) => tab.id);
      if (JSON.stringify(remainingZellijIds) !== JSON.stringify(survivingIds)) {
        throw new Error(`남은 zellij tab이 기대와 다르다: zellij=${remainingZellijIds}, expected=${survivingIds}`);
      }
      await takeScreenshot(page, run, "zellij-middle-tab-close-keeps-ids", screenshots);
      return `closed=${middleTab.id}, remaining=${remainingZellijIds.join(",")}`;
    });

    await optionalStep(checks, "요구사항① zellij 탭 순서 변경은 활성 탭을 바꾸지 않는다", async () => {
      const before = await readTabs(page);
      if (before.length < 2) throw new Error("순서 변경에는 탭 2개가 필요하다");
      const activeIdBefore = before.find((tab) => tab.isActive)?.id;

      await page.locator(`[data-terminal-tab-id='${before[0].id}']`)
        .dragTo(page.locator(`[data-terminal-tab-id='${before[1].id}']`));
      const tabs = await waitForTabs(
        page,
        (list) => list[0].id === before[1].id && list[1].id === before[0].id,
        "zellij 탭 순서가 바뀌지 않았다",
      );

      const activeIdAfter = tabs.find((tab) => tab.isActive)?.id;
      if (activeIdAfter !== activeIdBefore) {
        throw new Error(`순서만 바꿨는데 활성 탭이 바뀌었다: ${activeIdBefore} -> ${activeIdAfter}`);
      }
      await takeScreenshot(page, run, "zellij-tab-reordered", screenshots);
      return `order=[${tabs.map((tab) => tab.name)}], active 유지=${activeIdAfter}`;
    });

    await optionalStep(checks, "D7 KanVibe가 만든 tmux 세션은 자체 상태바를 끈다", async () => {
      const statusOption = execTmux(["show-options", "-t", tmuxSessionName, "status"]);
      if (!/^status off$/.test(statusOption)) {
        throw new Error(`세션 상태바가 꺼져 있지 않다: ${statusOption}`);
      }
      return statusOption;
    });

    await optionalStep(checks, "차단할 콘솔 에러가 없다", async () => {
      const blocking = consoleErrors.filter((line) => !/favicon|DevTools|Electron Security Warning|Insecure Content-Security-Policy|Autofill/i.test(line));
      if (blocking.length > 0) throw new Error(blocking.join("\n"));
      return "none";
    });
  } catch (error) {
    checks.push({
      name: "Electron 터미널 탭 QA 플로우",
      ok: false,
      detail: error instanceof Error ? error.stack || error.message : String(error),
    });
  } finally {
    if (page && traceStarted) {
      await page.context().tracing.stop({ path: run.tracePath }).catch((error) => {
        checks.push({ name: "Playwright trace artifact", ok: false, detail: error instanceof Error ? error.message : String(error) });
      });
    }
    /**
     * 세션 정리를 앱 종료보다 먼저 한다.
     * 멀티플렉서 서버는 Electron이 띄운 PTY의 stdio를 물고 살아남아,
     * 먼저 종료를 기다리면 파이프가 닫히지 않아 `app.close()`가 끝나지 않는다.
     */
    if (tmuxSessionName) {
      try {
        execFileSync("tmux", ["kill-session", "-t", tmuxSessionName], {
          stdio: "ignore",
          timeout: SESSION_CLEANUP_TIMEOUT_MS,
        });
      } catch {
        // 세션이 이미 사라진 경우 무시
      }
    }
    if (zellijSessionName) {
      /** zellij 세션 정리는 서버가 남아 있을 때 응답하지 않는 경우가 있어 상한을 둔다 */
      for (const command of ["kill-session", "delete-session"]) {
        try {
          execFileSync("zellij", [command, zellijSessionName], {
            stdio: "ignore",
            timeout: SESSION_CLEANUP_TIMEOUT_MS,
          });
        } catch {
          // 세션이 이미 사라졌거나 응답하지 않는 경우 무시
        }
      }
    }
    if (app) await app.close().catch(() => {});
  }

  await optionalStep(checks, "Playwright trace 아티팩트가 기록됐다", async () => {
    if (!fs.existsSync(run.tracePath) || fs.statSync(run.tracePath).size === 0) {
      throw new Error(`trace missing or empty: ${run.tracePath}`);
    }
    return run.tracePath;
  });

  await optionalStep(checks, "격리된 app-data 데이터베이스가 run 디렉터리에 쓰였다", async () => {
    const isolatedDatabasePath = path.join(run.runDir, "app-data", "kanvibe.db");
    if (!fs.existsSync(isolatedDatabasePath) || fs.statSync(isolatedDatabasePath).size === 0) {
      throw new Error(`isolated QA database missing or empty: ${isolatedDatabasePath}`);
    }
    return isolatedDatabasePath;
  });

  const ok = checks.every((check) => check.ok);
  const videoExists = fs.existsSync(run.videoPath) && fs.statSync(run.videoPath).size > 0;
  notes.push(`Fixture project: ${fixture.projectName} / ${fixture.projectDir}`);
  notes.push("QA는 사용자 DB가 아니라 run 디렉터리 안의 격리된 KANVIBE_APP_DATA_DIR을 쓴다.");
  notes.push(`Mod 키: ${MOD_KEY} (platform=${process.platform})`);
  notes.push(videoExists ? "ffmpeg로 실제 X11 화면을 녹화했다." : "플로우 종료 시점에 mp4가 없었다. wrapper가 스크린샷으로 합성할 수 있다.");
  notes.push(`Node: ${process.version}; platform: ${process.platform}/${process.arch}; tmp: ${os.tmpdir()}`);

  const result = {
    ok,
    scope: TERMINAL_TAB_SCOPE,
    branch: gitValue(["branch", "--show-current"]),
    commit: gitValue(["rev-parse", "--short", "HEAD"]),
    checks,
    errors: consoleErrors,
    screenshots,
    videoPath: videoExists ? run.videoPath : null,
    tracePath: fs.existsSync(run.tracePath) ? run.tracePath : null,
    notes,
  };

  writeReport(run, result);
  console.log(`[kanvibe-qa] terminal-tab report: ${run.reportPath}`);
  process.exit(ok ? 0 : 1);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
