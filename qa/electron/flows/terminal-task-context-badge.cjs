#!/usr/bin/env node
/* eslint-disable @typescript-eslint/no-require-imports */
/**
 * 터미널 헤더 태스크 배지 QA 플로우.
 * 배지 배경이 프로젝트 설정 색을 그대로 쓰는지, 그리고 그 배경 위에 얹은 첫 글자 칩이
 * 같은 색으로 묻히지 않는지를 실제 Electron 화면의 계산된 스타일로 확인한다.
 * fixture 저장소에는 GitHub remote를 두지 않아 항상 이니셜 칩 폴백 경로를 타게 만든다.
 */
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const { createQaRun } = require("../lib/report.cjs");
const { launchKanVibeElectron } = require("../lib/launchElectron.cjs");

const TASK_CONTEXT_BADGE_SCOPE = "터미널 헤더 태스크 배지: 프로젝트 아이콘·이름·태스크명이 한 배지에 순서대로 나오고, 배경이 프로젝트 설정 색과 같으며, 밝은 색과 어두운 색 모두에서 첫 글자 칩과 글자가 배경 위에서 읽히는지 검증한다";

/** 어두운 프로젝트 색과 밝은 프로젝트 색 양쪽에서 대비 반전이 도는지 봐야 한다 */
const DARK_PROJECT_COLOR = "#0064FF";
const LIGHT_PROJECT_COLOR = "#FDE047";

/** WCAG 1.4.11 non-text contrast. 칩은 배경 위에 얹힌 그래픽이므로 이 선을 넘어야 보인다 */
const MINIMUM_GRAPHIC_CONTRAST = 3;
/** WCAG 1.4.3 large text. 배지 글자는 작으므로 여유를 두고 이 선으로 본다 */
const MINIMUM_TEXT_CONTRAST = 4.5;

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

function writeJson(filePath, data) {
  fs.writeFileSync(filePath, `${JSON.stringify(data, null, 2)}\n`, "utf8");
}

function ensureDir(dirPath) {
  fs.mkdirSync(dirPath, { recursive: true });
}

function renderReport(result) {
  const lines = [];
  lines.push("# KanVibe Terminal Task Context Badge Electron QA Report");
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

/** GitHub remote가 없어야 아이콘 대신 이니셜 칩 폴백이 그려진다 */
function createLocalFixtureRepository(run) {
  const projectDir = path.join(run.runDir, "fixtures", `task-context-badge-${run.runId}`);
  const projectName = `Quasar Badge QA ${run.runId}`;
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

/** 기본으로 열리는 작업 정보 패널은 터미널 크롬을 덮으므로 패널 밖 본문을 눌러 닫는다 */
async function dismissDetailPanel(page) {
  const viewport = page.viewportSize() || { width: 1500, height: 950 };
  await page.mouse.click(Math.round(viewport.width * 0.75), Math.round(viewport.height * 0.6));
  await page.waitForTimeout(500);
}

async function openTaskDetail(page, taskId) {
  await page.evaluate((id) => {
    window.location.hash = `#/ko/task/${id}`;
  }, taskId);
  await page.getByTestId("terminal-task-context").waitFor({ state: "visible", timeout: 20000 });
  await dismissDetailPanel(page);
  await page.waitForTimeout(1000);
}

/** 색을 바꾼 뒤 라우트가 태스크를 다시 읽게 만든다 */
async function reopenTaskDetail(page, taskId) {
  await page.evaluate(() => {
    window.location.hash = "#/ko";
  });
  await page.waitForTimeout(800);
  await openTaskDetail(page, taskId);
}

function parseRgb(cssColor) {
  const matched = String(cssColor).match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
  if (!matched) throw new Error(`계산된 색을 읽을 수 없다: ${cssColor}`);
  return [Number(matched[1]), Number(matched[2]), Number(matched[3])];
}

function toLinearChannel(channel) {
  const normalized = channel / 255;
  return normalized <= 0.03928 ? normalized / 12.92 : ((normalized + 0.055) / 1.055) ** 2.4;
}

function relativeLuminance(rgb) {
  const weights = [0.2126, 0.7152, 0.0722];
  return rgb.reduce((total, channel, index) => total + toLinearChannel(channel) * weights[index], 0);
}

/** WCAG 대비율. 프로덕션이 고른 색을 그대로 믿지 않고 결과를 독립적으로 재는 데 쓴다 */
function contrastRatio(foreground, background) {
  const foregroundLuminance = relativeLuminance(foreground);
  const backgroundLuminance = relativeLuminance(background);
  const lighter = Math.max(foregroundLuminance, backgroundLuminance);
  const darker = Math.min(foregroundLuminance, backgroundLuminance);
  return (lighter + 0.05) / (darker + 0.05);
}

function hexToRgb(hexColor) {
  const value = Number.parseInt(hexColor.replace("#", ""), 16);
  return [(value >> 16) & 0xff, (value >> 8) & 0xff, value & 0xff];
}

function isSameColor(a, b) {
  return a[0] === b[0] && a[1] === b[1] && a[2] === b[2];
}

/** 배지 한 덩어리에서 판정에 필요한 텍스트와 계산된 색을 한 번에 읽어 온다 */
async function readBadgeAppearance(page) {
  return page.evaluate(() => {
    const badge = document.querySelector('[data-testid="terminal-task-context"]');
    if (!badge) return null;

    const badgeStyle = window.getComputedStyle(badge);
    const projectNameNode = badge.querySelector('[data-testid="terminal-task-context-project"]');
    const initialChip = badge.querySelector('[data-testid="project-initial-icon"]');
    const githubIcon = badge.querySelector('[data-testid="project-github-icon"]');
    const chipStyle = initialChip ? window.getComputedStyle(initialChip) : null;

    return {
      text: badge.textContent,
      title: badge.getAttribute("title"),
      backgroundColor: badgeStyle.backgroundColor,
      textColor: badgeStyle.color,
      projectName: projectNameNode ? projectNameNode.textContent : null,
      hasInitialChip: Boolean(initialChip),
      hasGitHubIcon: Boolean(githubIcon),
      chipBackgroundColor: chipStyle ? chipStyle.backgroundColor : null,
      chipTextColor: chipStyle ? chipStyle.color : null,
      /** 배지 안에서 실제로 보이는 순서를 확인하려고 자식 노드를 순서대로 남긴다 */
      childOrder: [...badge.childNodes].map((node) => (
        node.nodeType === Node.TEXT_NODE
          ? `#text:${node.textContent.trim()}`
          : `${node.tagName.toLowerCase()}:${node.getAttribute?.("data-testid") || node.textContent.trim()}`
      )),
    };
  });
}

/** 색 변경이 라우트에 반영될 때까지 기다린다 */
async function waitForBadgeBackground(page, expectedHex, timeoutMs = 20000) {
  const expected = hexToRgb(expectedHex);
  const deadline = Date.now() + timeoutMs;
  let lastAppearance = null;

  while (Date.now() < deadline) {
    lastAppearance = await readBadgeAppearance(page);
    if (lastAppearance && isSameColor(parseRgb(lastAppearance.backgroundColor), expected)) {
      return lastAppearance;
    }
    await page.waitForTimeout(400);
  }

  throw new Error(`배지 배경이 ${expectedHex}로 바뀌지 않았다: ${JSON.stringify(lastAppearance)}`);
}

/** 배지가 밝은/어두운 프로젝트 색 어느 쪽에서도 읽히는지 한 묶음으로 판정한다 */
function assertBadgeIsLegible(appearance, expectedHex) {
  const background = parseRgb(appearance.backgroundColor);
  const text = parseRgb(appearance.textColor);
  const failures = [];

  const textContrast = contrastRatio(text, background);
  if (textContrast < MINIMUM_TEXT_CONTRAST) {
    failures.push(`글자 대비 ${textContrast.toFixed(2)}:1 < ${MINIMUM_TEXT_CONTRAST}:1`);
  }

  if (!appearance.hasInitialChip) {
    failures.push("이니셜 칩이 그려지지 않았다");
  } else {
    const chipBackground = parseRgb(appearance.chipBackgroundColor);
    if (isSameColor(chipBackground, background)) {
      failures.push("이니셜 칩 배경이 배지 배경과 같아 묻힌다");
    }
    const chipContrast = contrastRatio(chipBackground, background);
    if (chipContrast < MINIMUM_GRAPHIC_CONTRAST) {
      failures.push(`칩 대비 ${chipContrast.toFixed(2)}:1 < ${MINIMUM_GRAPHIC_CONTRAST}:1`);
    }
    const chipTextContrast = contrastRatio(parseRgb(appearance.chipTextColor), chipBackground);
    if (chipTextContrast < MINIMUM_TEXT_CONTRAST) {
      failures.push(`칩 글자 대비 ${chipTextContrast.toFixed(2)}:1 < ${MINIMUM_TEXT_CONTRAST}:1`);
    }
  }

  if (failures.length > 0) {
    throw new Error(`${expectedHex} 배경에서 배지가 읽히지 않는다: ${failures.join(", ")}`);
  }

  const chipBackground = parseRgb(appearance.chipBackgroundColor);
  return [
    `bg=${appearance.backgroundColor}`,
    `text=${appearance.textColor} (${contrastRatio(text, background).toFixed(2)}:1)`,
    `chip=${appearance.chipBackgroundColor} (${contrastRatio(chipBackground, background).toFixed(2)}:1)`,
  ].join(", ");
}

async function seedBadgeFixture(page, fixture, runId) {
  const projectResult = await invokeDesktop(page, "project", "registerProject", fixture.projectName, fixture.projectDir);
  if (!projectResult.success || !projectResult.project) {
    throw new Error(projectResult.error || "QA project registration failed");
  }

  const project = projectResult.project;
  const task = await invokeDesktop(page, "kanban", "createTask", {
    title: `qa-badge-project-marker-${runId}`,
    description: "Electron QA task for the terminal header project marker badge.",
    projectId: project.id,
    baseBranch: fixture.defaultBranch,
  });

  const connected = await invokeDesktop(page, "kanban", "connectTerminalSession", task.id, "terminal");
  if (!connected || connected.sessionType !== "terminal") {
    throw new Error(`connectTerminalSession failed: ${JSON.stringify(connected)}`);
  }

  return { project, taskId: task.id, taskTitle: task.title };
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
    await optionalStep(checks, "GitHub remote 없는 QA 프로젝트와 터미널 태스크를 준비한다", async () => {
      seeded = await seedBadgeFixture(page, fixture, run.runId);
      if (seeded.project.iconDataUrl) {
        throw new Error("fixture 프로젝트에 GitHub 아이콘이 붙어 이니셜 칩 폴백을 검증할 수 없다");
      }
      return `project=${seeded.project.name}, task=${seeded.taskId}`;
    });
    if (!seeded) throw new Error("QA seed 실패로 배지 플로우를 진행할 수 없다");

    await optionalStep(checks, "어두운 프로젝트 색을 지정하면 배지 배경이 그 색과 같아진다", async () => {
      await invokeDesktop(page, "kanban", "updateProjectColor", seeded.project.id, DARK_PROJECT_COLOR);
      await openTaskDetail(page, seeded.taskId);
      const appearance = await waitForBadgeBackground(page, DARK_PROJECT_COLOR);
      await takeScreenshot(page, run, "badge-dark-project-color", screenshots);
      return `expected=${DARK_PROJECT_COLOR}, actual=${appearance.backgroundColor}`;
    });

    await optionalStep(checks, "배지가 아이콘·프로젝트명·구분자·태스크명 순서로 나온다", async () => {
      const appearance = await readBadgeAppearance(page);
      if (appearance.projectName !== seeded.project.name) {
        throw new Error(`프로젝트명이 배지에 없다: ${JSON.stringify(appearance.projectName)}`);
      }
      if (!appearance.text.includes(seeded.taskTitle)) {
        throw new Error(`태스크명이 배지에 없다: ${appearance.text}`);
      }
      if (appearance.title !== `${seeded.project.name} | ${seeded.taskTitle}`) {
        throw new Error(`tooltip이 프로젝트명과 태스크명을 함께 담지 않았다: ${appearance.title}`);
      }
      const chipIndex = appearance.childOrder.findIndex((entry) => entry.includes("project-initial-icon"));
      const nameIndex = appearance.childOrder.findIndex((entry) => entry.includes("terminal-task-context-project"));
      if (chipIndex === -1 || nameIndex === -1 || chipIndex > nameIndex) {
        throw new Error(`아이콘이 프로젝트명 왼쪽에 있지 않다: ${JSON.stringify(appearance.childOrder)}`);
      }
      return `order=${appearance.childOrder.join(" > ")}`;
    });

    await optionalStep(checks, "어두운 프로젝트 색 위에서 글자와 이니셜 칩이 모두 읽힌다", async () => {
      const appearance = await readBadgeAppearance(page);
      return assertBadgeIsLegible(appearance, DARK_PROJECT_COLOR);
    });

    await optionalStep(checks, "밝은 프로젝트 색으로 바꾸면 배경이 따라가고 대비가 반전된다", async () => {
      const darkAppearance = await readBadgeAppearance(page);
      await invokeDesktop(page, "kanban", "updateProjectColor", seeded.project.id, LIGHT_PROJECT_COLOR);
      await reopenTaskDetail(page, seeded.taskId);
      const lightAppearance = await waitForBadgeBackground(page, LIGHT_PROJECT_COLOR);
      await takeScreenshot(page, run, "badge-light-project-color", screenshots);

      if (lightAppearance.textColor === darkAppearance.textColor) {
        throw new Error(`밝은 색으로 바꿔도 글자색이 그대로다: ${lightAppearance.textColor}`);
      }
      const legibility = assertBadgeIsLegible(lightAppearance, LIGHT_PROJECT_COLOR);
      return `${legibility}, 어두운 색일 때 글자=${darkAppearance.textColor}`;
    });

    await optionalStep(checks, "차단성 콘솔 에러가 없다", async () => {
      const blocking = consoleErrors.filter((line) => !/favicon|DevTools|Electron Security Warning|Insecure Content-Security-Policy/i.test(line));
      if (blocking.length > 0) throw new Error(blocking.join("\n"));
      return "none";
    });
  } catch (error) {
    checks.push({ name: "Electron 터미널 배지 QA 플로우", ok: false, detail: error instanceof Error ? error.stack || error.message : String(error) });
  } finally {
    if (page && traceStarted) {
      await page.context().tracing.stop({ path: run.tracePath }).catch((error) => {
        checks.push({ name: "Playwright trace artifact", ok: false, detail: error instanceof Error ? error.message : String(error) });
      });
    }
    if (app) await app.close().catch(() => {});
  }

  await optionalStep(checks, "Playwright trace artifact written", async () => {
    if (!fs.existsSync(run.tracePath) || fs.statSync(run.tracePath).size === 0) {
      throw new Error(`trace missing or empty: ${run.tracePath}`);
    }
    return run.tracePath;
  });

  const ok = checks.every((check) => check.ok);
  const videoExists = fs.existsSync(run.videoPath) && fs.statSync(run.videoPath).size > 0;
  notes.push(`Fixture project: ${fixture.projectName} (GitHub remote 없음 → 이니셜 칩 폴백 경로)`);
  notes.push("QA uses an isolated KANVIBE_APP_DATA_DIR inside the run directory, not the user's app database.");
  notes.push(videoExists ? "Actual X11 screen recording captured with ffmpeg." : "No mp4 was present when flow finished; wrapper script can synthesize one from screenshots.");
  notes.push(`Node: ${process.version}; platform: ${process.platform}/${process.arch}; tmp: ${os.tmpdir()}`);

  const result = {
    ok,
    scope: TASK_CONTEXT_BADGE_SCOPE,
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
  console.log(JSON.stringify({ ok, runDir: run.runDir, reportPath: run.reportPath, videoPath: run.videoPath }, null, 2));
  process.exit(ok ? 0 : 1);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
