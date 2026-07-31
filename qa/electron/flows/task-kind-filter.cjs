#!/usr/bin/env node
/* eslint-disable @typescript-eslint/no-require-imports */
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const { createQaRun } = require("../lib/report.cjs");
const { launchKanVibeElectron } = require("../lib/launchElectron.cjs");

const TASK_KIND_FILTER_SCOPE = "Project/Task/All task-kind filter UI: seed an isolated project-root task plus branch/plain tasks, then verify the Board filter buttons hide/show the expected cards in Electron";

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
  lines.push("# KanVibe Task Kind Filter Electron QA Report");
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
  const projectDir = path.join(run.runDir, "fixtures", `task-kind-filter-${run.runId}`);
  const projectName = `Task Kind Filter QA ${run.runId}`;
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

function cardSelector(taskId) {
  return `[data-kanban-task-card='true'][data-kanban-task-id='${taskId}']`;
}

async function expectCardVisible(page, taskId, label) {
  const card = page.locator(cardSelector(taskId));
  await card.waitFor({ state: "visible", timeout: 15000 });
  await card.scrollIntoViewIfNeeded();
  return `${label}:${taskId}`;
}

async function expectCardHidden(page, taskId, label) {
  await page.waitForFunction(
    (id) => !document.querySelector(`[data-kanban-task-card='true'][data-kanban-task-id='${id}']`),
    taskId,
    { timeout: 15000 },
  );
  return `${label}:${taskId}`;
}

async function expectVisibleAndHidden(page, visible, hidden) {
  const visibleDetails = [];
  for (const item of visible) {
    visibleDetails.push(await expectCardVisible(page, item.id, item.label));
  }
  const hiddenDetails = [];
  for (const item of hidden) {
    hiddenDetails.push(await expectCardHidden(page, item.id, item.label));
  }
  return `visible=[${visibleDetails.join(", ")}], hidden=[${hiddenDetails.join(", ")}]`;
}

async function clickTaskKindFilter(page, name) {
  await page.getByTestId("task-kind-filter").getByRole("button", { name }).click();
  await page.waitForTimeout(900);
}

async function seedTaskKindFilterData(page, fixture, runId) {
  const projectResult = await invokeDesktop(page, "project", "registerProject", fixture.projectName, fixture.projectDir);
  if (!projectResult.success || !projectResult.project) {
    throw new Error(projectResult.error || "QA project registration failed");
  }

  const project = projectResult.project;
  const rootTaskId = await invokeDesktop(page, "kanban", "getTaskIdByProjectAndBranch", project.id, fixture.defaultBranch);
  if (!rootTaskId) {
    throw new Error("Project root task was not created for the fixture project");
  }

  const branchTask = await invokeDesktop(page, "kanban", "createTask", {
    title: `qa-filter-branch-${runId}`,
    description: "Electron QA branch task for the Project/Task/All filter.",
    branchName: `qa/filter-${runId}`,
    baseBranch: fixture.defaultBranch,
    projectId: project.id,
  });
  const plainTask = await invokeDesktop(page, "kanban", "createTask", {
    title: `qa-filter-plain-${runId}`,
    description: "Electron QA plain task for the Project/Task/All filter.",
    projectId: project.id,
  });

  await page.evaluate(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
    window.location.hash = "#/ko";
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await dismissUpdateDialogIfPresent(page);

  return {
    project,
    rootTask: { id: rootTaskId, label: "project-root" },
    branchTask: { id: branchTask.id, label: "branch-task" },
    plainTask: { id: plainTask.id, label: "plain-task" },
  };
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
    await optionalStep(checks, "Seed isolated project-root, branch, and plain tasks through app IPC", async () => {
      seeded = await seedTaskKindFilterData(page, fixture, run.runId);
      return `project=${seeded.project.name}, root=${seeded.rootTask.id}, branch=${seeded.branchTask.id}, plain=${seeded.plainTask.id}`;
    });
    if (!seeded) throw new Error("QA seed failed; cannot continue task-kind filter flow");

    await optionalStep(checks, "Task kind filter is left of the project selector with matching control sizing", async () => {
      await page.getByTestId("task-kind-filter").waitFor({ state: "visible", timeout: 15000 });
      await page.getByTestId("project-filter-control").waitFor({ state: "visible", timeout: 15000 });
      const metrics = await page.evaluate(() => {
        const filter = document.querySelector("[data-testid='task-kind-filter']");
        const project = document.querySelector("[data-testid='project-filter-control']");
        if (!filter || !project) return null;
        const filterRect = filter.getBoundingClientRect();
        const projectRect = project.getBoundingClientRect();
        return {
          filterLeft: filterRect.left,
          filterRight: filterRect.right,
          filterWidth: filterRect.width,
          filterHeight: filterRect.height,
          projectLeft: projectRect.left,
          projectHeight: projectRect.height,
          filterClassName: filter.className,
        };
      });
      if (!metrics) throw new Error("filter/project selector controls were not found");
      if (metrics.filterRight > metrics.projectLeft) {
        throw new Error(`task-kind filter is not left of project selector: filterRight=${metrics.filterRight}, projectLeft=${metrics.projectLeft}`);
      }
      if (Math.abs(metrics.filterHeight - metrics.projectHeight) > 2) {
        throw new Error(`control heights differ: filter=${metrics.filterHeight}, project=${metrics.projectHeight}`);
      }
      if (Math.abs(metrics.filterWidth - 180) > 2 || Math.abs(metrics.filterHeight - 34) > 2) {
        throw new Error(`unexpected task-kind filter size: ${metrics.filterWidth}x${metrics.filterHeight}`);
      }
      if (!String(metrics.filterClassName).includes("bg-bg-page")) {
        throw new Error(`task-kind filter is missing bg-bg-page styling: ${metrics.filterClassName}`);
      }
      return `filter=${Math.round(metrics.filterWidth)}x${Math.round(metrics.filterHeight)} left of project selector (${Math.round(metrics.filterRight)} <= ${Math.round(metrics.projectLeft)})`;
    });

    await optionalStep(checks, "All filter shows project root, branch task, and plain task", async () => {
      await clickTaskKindFilter(page, "All");
      const detail = await expectVisibleAndHidden(
        page,
        [seeded.rootTask, seeded.branchTask, seeded.plainTask],
        [],
      );
      await takeScreenshot(page, run, "all-filter-shows-root-branch-plain", screenshots);
      return detail;
    });

    await optionalStep(checks, "Project filter shows only the project-root task", async () => {
      await clickTaskKindFilter(page, "Project");
      const detail = await expectVisibleAndHidden(
        page,
        [seeded.rootTask],
        [seeded.branchTask, seeded.plainTask],
      );
      await takeScreenshot(page, run, "project-filter-shows-root-only", screenshots);
      return detail;
    });

    await optionalStep(checks, "Task filter hides project root and shows branch/plain tasks", async () => {
      await clickTaskKindFilter(page, "Task");
      const detail = await expectVisibleAndHidden(
        page,
        [seeded.branchTask, seeded.plainTask],
        [seeded.rootTask],
      );
      await takeScreenshot(page, run, "task-filter-hides-root", screenshots);
      return detail;
    });

    await optionalStep(checks, "Filter buttons keep their pressed state visible", async () => {
      const taskPressed = await page.getByTestId("task-kind-filter").getByRole("button", { name: "Task" }).getAttribute("aria-pressed");
      await clickTaskKindFilter(page, "All");
      const allPressed = await page.getByTestId("task-kind-filter").getByRole("button", { name: "All" }).getAttribute("aria-pressed");
      if (taskPressed !== "true" || allPressed !== "true") {
        throw new Error(`unexpected pressed states: Task=${taskPressed}, All=${allPressed}`);
      }
      await takeScreenshot(page, run, "all-filter-restored", screenshots);
      return "Task and All buttons expose aria-pressed=true when active";
    });

    await optionalStep(checks, "No blocking console errors", async () => {
      const blocking = consoleErrors.filter((line) => !/favicon|DevTools|Electron Security Warning|Insecure Content-Security-Policy/i.test(line));
      if (blocking.length > 0) throw new Error(blocking.join("\n"));
      return "none";
    });
  } catch (error) {
    checks.push({ name: "Electron task-kind filter QA flow", ok: false, detail: error instanceof Error ? error.stack || error.message : String(error) });
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
  notes.push(`Fixture project: ${fixture.projectName} / ${fixture.projectDir}`);
  notes.push("QA uses an isolated KANVIBE_APP_DATA_DIR inside the run directory, not the user's app database.");
  notes.push(videoExists ? "Actual X11 screen recording captured with ffmpeg." : "No mp4 was present when flow finished; wrapper script can synthesize one from screenshots.");
  notes.push(`Node: ${process.version}; platform: ${process.platform}/${process.arch}; tmp: ${os.tmpdir()}`);

  const result = {
    ok,
    scope: TASK_KIND_FILTER_SCOPE,
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
