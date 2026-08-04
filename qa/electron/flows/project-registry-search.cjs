#!/usr/bin/env node
/* eslint-disable @typescript-eslint/no-require-imports */
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const { createQaRun } = require("../lib/report.cjs");
const { launchKanVibeElectron } = require("../lib/launchElectron.cjs");
const koMessages = require("../../../messages/ko.json");

const PROJECT_REGISTRY_SEARCH_SCOPE = "Project scan button and registered-project search filter: verify the board toolbar button reads as a project/folder scan action, then seed three isolated projects and filter the registry dialog list by name in Electron";
const SCAN_BUTTON_LABEL = koMessages.settings.scanTitle;
const NO_MATCHING_PROJECTS_TEXT = koMessages.settings.noMatchingProjects;

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
  lines.push("# KanVibe Project Scan Button / Registry Search Electron QA Report");
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

function createLocalFixtureRepository(run, slug) {
  const projectDir = path.join(run.runDir, "fixtures", `registry-search-${run.runId}`, slug);
  const projectName = `${slug}-${run.runId}`;
  fs.rmSync(projectDir, { recursive: true, force: true });
  ensureDir(projectDir);

  try {
    execGit(["init", "--initial-branch", "main"], projectDir);
  } catch {
    execGit(["init"], projectDir);
    execGit(["checkout", "-B", "main"], projectDir);
  }
  execGit(["config", "user.email", "qa@kanvibe.local"], projectDir);
  execGit(["config", "user.name", "KanVibe QA"], projectDir);
  fs.writeFileSync(path.join(projectDir, "README.md"), `# ${projectName}\n`, "utf8");
  execGit(["add", "README.md"], projectDir);
  execGit(["commit", "-m", "Initial QA fixture"], projectDir);

  return { projectDir, projectName, slug };
}

function registryDialog(page) {
  return page.getByRole("dialog", { name: SCAN_BUTTON_LABEL });
}

function projectRows(page) {
  return registryDialog(page).locator("ul > li");
}

/** 각 프로젝트 행의 첫 단락(이름)만 읽는다. repoPath 단락과 섞이지 않게 행 단위로 순회한다 */
async function visibleProjectNames(page) {
  const rows = projectRows(page);
  const rowCount = await rows.count();
  const names = [];
  for (let index = 0; index < rowCount; index += 1) {
    names.push((await rows.nth(index).locator("p").first().innerText()).trim());
  }
  return names;
}

async function projectRowCount(page) {
  return projectRows(page).count();
}

async function typeProjectSearch(page, query) {
  const input = page.getByTestId("project-registry-search");
  await input.fill(query);
  await page.waitForTimeout(600);
}

async function seedProjects(page, fixtures) {
  const registered = [];
  for (const fixture of fixtures) {
    const result = await invokeDesktop(page, "project", "registerProject", fixture.projectName, fixture.projectDir);
    if (!result.success || !result.project) {
      throw new Error(result.error || `QA project registration failed for ${fixture.projectName}`);
    }
    registered.push({ ...fixture, project: result.project });
  }

  await page.evaluate(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
    window.location.hash = "#/ko";
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await dismissUpdateDialogIfPresent(page);

  return registered;
}

async function main() {
  const options = parseArgs(process.argv);
  const run = createQaRun({ ...options, rootDir: process.cwd() });
  const fixtures = ["alpha", "bravo", "charlie"].map((slug) => createLocalFixtureRepository(run, slug));
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
    page.on("requestfailed", (request) => {
      const failure = request.failure();
      consoleErrors.push(`requestfailed: ${request.url()} ${failure?.errorText || "unknown"}`);
    });

    await page.context().tracing.start({ screenshots: true, snapshots: true, sources: true });
    traceStarted = true;

    await optionalStep(checks, "Electron window opens on the KanVibe board", async () => {
      await page.waitForLoadState("domcontentloaded");
      await page.locator("body").waitFor({ state: "visible", timeout: 20000 });
      await dismissUpdateDialogIfPresent(page);
      return page.url();
    });

    let seeded = null;
    await optionalStep(checks, "Seed three isolated projects through app IPC", async () => {
      seeded = await seedProjects(page, fixtures);
      return seeded.map((entry) => entry.projectName).join(", ");
    });
    if (!seeded) throw new Error("QA seed failed; cannot continue project registry search flow");

    await optionalStep(checks, "Toolbar scan button reads as a project scan action, not a search action", async () => {
      const button = page.getByRole("button", { name: SCAN_BUTTON_LABEL });
      await button.waitFor({ state: "visible", timeout: 15000 });
      const icon = await page.evaluate((label) => {
        const target = document.querySelector(`button[aria-label="${label}"]`);
        if (!target) return null;
        const svg = target.querySelector("svg");
        if (!svg) return null;
        return {
          title: target.getAttribute("title"),
          circleCount: svg.querySelectorAll("circle").length,
          paths: Array.from(svg.querySelectorAll("path")).map((node) => node.getAttribute("d") || ""),
        };
      }, SCAN_BUTTON_LABEL);
      if (!icon) throw new Error("scan button or its icon was not found");
      if (icon.title !== SCAN_BUTTON_LABEL) {
        throw new Error(`unexpected scan button tooltip: ${icon.title}`);
      }
      if (icon.circleCount > 0) {
        throw new Error(`scan button still renders a magnifier-style circle (${icon.circleCount})`);
      }
      if (icon.paths.length === 0) {
        throw new Error("scan button renders no icon path");
      }
      await takeScreenshot(page, run, "toolbar-project-scan-button", screenshots);
      return `title/aria-label="${icon.title}", icon paths=${icon.paths.length}, circles=0`;
    });

    await optionalStep(checks, "Scan button opens the project registry dialog listing all seeded projects", async () => {
      await page.getByRole("button", { name: SCAN_BUTTON_LABEL }).click();
      await registryDialog(page).waitFor({ state: "visible", timeout: 15000 });
      await page.getByTestId("project-registry-search").waitFor({ state: "visible", timeout: 15000 });
      const rows = await projectRowCount(page);
      if (rows !== fixtures.length) {
        throw new Error(`expected ${fixtures.length} project rows, found ${rows}`);
      }
      await takeScreenshot(page, run, "registry-dialog-all-projects", screenshots);
      return `dialog title="${SCAN_BUTTON_LABEL}", rows=${rows}`;
    });

    await optionalStep(checks, "Search input filters the registered project list down to the matching project", async () => {
      await typeProjectSearch(page, "bravo");
      const rows = await projectRowCount(page);
      const names = await visibleProjectNames(page);
      if (rows !== 1) throw new Error(`expected 1 matching row, found ${rows}: ${names.join(", ")}`);
      if (!names.some((name) => name.includes("bravo"))) {
        throw new Error(`matching row is not the bravo project: ${names.join(", ")}`);
      }
      const heading = await registryDialog(page).locator("h3").innerText();
      if (!heading.includes(`(1 / ${fixtures.length})`)) {
        throw new Error(`heading count did not follow the filter: ${heading}`);
      }
      await takeScreenshot(page, run, "search-filters-to-single-project", screenshots);
      return `query="bravo", rows=${rows}, heading="${heading.trim()}"`;
    });

    await optionalStep(checks, "Search with no match shows the empty-result notice instead of the list", async () => {
      await typeProjectSearch(page, "존재하지-않는-프로젝트");
      const rows = await projectRowCount(page);
      if (rows !== 0) throw new Error(`expected 0 rows for a non-matching query, found ${rows}`);
      await registryDialog(page).getByText(NO_MATCHING_PROJECTS_TEXT).waitFor({ state: "visible", timeout: 10000 });
      await takeScreenshot(page, run, "search-no-match-notice", screenshots);
      return "empty-result notice visible with 0 rows";
    });

    await optionalStep(checks, "Clearing the search restores every registered project", async () => {
      await typeProjectSearch(page, "");
      const rows = await projectRowCount(page);
      if (rows !== fixtures.length) {
        throw new Error(`expected ${fixtures.length} rows after clearing the query, found ${rows}`);
      }
      await takeScreenshot(page, run, "search-cleared-restores-list", screenshots);
      return `rows=${rows}`;
    });

    await optionalStep(checks, "No blocking console errors", async () => {
      const blocking = consoleErrors.filter((line) => !/favicon|DevTools|Electron Security Warning|Insecure Content-Security-Policy/i.test(line));
      if (blocking.length > 0) throw new Error(blocking.join("\n"));
      return "none";
    });
  } catch (error) {
    checks.push({ name: "Electron project registry search QA flow", ok: false, detail: error instanceof Error ? error.stack || error.message : String(error) });
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

  await optionalStep(checks, "Isolated app-data database written inside run directory", async () => {
    const isolatedDatabasePath = path.join(run.runDir, "app-data", "kanvibe.db");
    if (!fs.existsSync(isolatedDatabasePath) || fs.statSync(isolatedDatabasePath).size === 0) {
      throw new Error(`isolated QA database missing or empty: ${isolatedDatabasePath}`);
    }
    return isolatedDatabasePath;
  });

  const ok = checks.every((check) => check.ok);
  const videoExists = fs.existsSync(run.videoPath) && fs.statSync(run.videoPath).size > 0;
  notes.push(`Fixture projects: ${fixtures.map((fixture) => fixture.projectName).join(", ")}`);
  notes.push("QA uses an isolated KANVIBE_APP_DATA_DIR inside the run directory, not the user's app database.");
  notes.push(videoExists ? "Actual X11 screen recording captured with ffmpeg." : "No mp4 was present when flow finished; wrapper script can synthesize one from screenshots.");
  notes.push(`Node: ${process.version}; platform: ${process.platform}/${process.arch}; tmp: ${os.tmpdir()}`);

  const result = {
    ok,
    scope: PROJECT_REGISTRY_SEARCH_SCOPE,
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
