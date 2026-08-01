#!/usr/bin/env node
/* eslint-disable @typescript-eslint/no-require-imports */
/**
 * docs용 보드 스크린샷 캡처 플로우.
 * GitHub remote가 있는 저장소(아이콘)와 없는 저장소(이니셜 배지)를 함께 시드해
 * 프로젝트 마커 두 종류가 한 화면에 나오게 한다.
 */
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const { launchKanVibeElectron } = require("../lib/launchElectron.cjs");

const ROOT_DIR = process.env.KANVIBE_ROOT_DIR;
const BOARD_VIEWPORT = { width: 1680, height: 1040 };
/** docs 이미지는 Retina 화면에서도 선명하도록 2배 밀도로 촬영한다 */
const DEVICE_SCALE_FACTOR = 2;
const OUT_DIR = process.env.CAPTURE_OUT_DIR;

/** owner가 있으면 GitHub remote를 붙여 아이콘을 받고, 없으면 이니셜 배지로 그려진다 */
const PROJECT_FIXTURES = [
  { name: "kanvibe", owner: "rookedsysc" },
  { name: "techtaurant-be", owner: "rookedsysc" },
  { name: "techtaurant-fe", owner: "rookedsysc" },
  { name: "prompt", owner: "rookedsysc" },
  { name: "llm-wiki-mcp", owner: "rookedsysc" },
  { name: "portfolio", owner: "rookedsysc" },
  { name: "nvim", owner: null },
  { name: "timelabs-mobile", owner: null },
  { name: "dotfiles", owner: null },
];

const TASK_FIXTURES = [
  { project: "kanvibe", title: "feat/project-icon", status: "progress", description: "보드 카드 프로젝트 마커를 아이콘으로 교체" },
  { project: "kanvibe", title: "modify/state-sync", status: "review", description: ".kanvibe 상태를 client 단위로 공유" },
  { project: "kanvibe", title: "fix/kanban-task-move", status: "done" },
  { project: "kanvibe", title: "feat/task-detail-dock-shortcut", status: "done" },
  { project: "techtaurant-be", title: "refactor/auth-detail", status: "review" },
  { project: "techtaurant-be", title: "modify/prepare-ssg-isr", status: "done", description: "ssg isr 위한 api 분리 작업" },
  { project: "techtaurant-fe", title: "feat/setting-redirect", status: "progress" },
  { project: "techtaurant-fe", title: "feat/comment-thread", status: "todo" },
  { project: "prompt", title: "feat/roky-harness", status: "todo" },
  { project: "llm-wiki-mcp", title: "feat/vault-push", status: "pending" },
  { project: "portfolio", title: "chore/deploy", status: "todo" },
  { project: "nvim", title: "feat/lsp-config", status: "todo" },
  { project: "timelabs-mobile", title: "feat/timesystem", status: "progress" },
  { project: "timelabs-mobile", title: "feat/project-screen", status: "pending" },
  { project: "dotfiles", title: "chore/zsh-plugins", status: "todo" },
];

function execGit(args, cwd) {
  return execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

function createLocalRepository(fixturesRoot, fixture) {
  const projectDir = path.join(fixturesRoot, fixture.name);
  fs.mkdirSync(projectDir, { recursive: true });

  execGit(["init", "--initial-branch=main"], projectDir);
  execGit(["config", "user.email", "qa@kanvibe.local"], projectDir);
  execGit(["config", "user.name", "KanVibe QA"], projectDir);
  fs.writeFileSync(path.join(projectDir, "README.md"), `# ${fixture.name}\n`, "utf8");
  execGit(["add", "."], projectDir);
  execGit(["commit", "-m", "chore: init"], projectDir);

  if (fixture.owner) {
    execGit(["remote", "add", "origin", `https://github.com/${fixture.owner}/${fixture.name}.git`], projectDir);
  }

  return projectDir;
}

/**
 * Playwright의 setViewportSize는 deviceScaleFactor를 1로 고정해 Retina 배율을 죽인다.
 * docs용 이미지는 2배 밀도가 필요하므로 CDP로 배율을 직접 지정한 뒤 촬영한다.
 */
async function captureRetinaScreenshot(app, page, shotPath) {
  const devicePixelRatio = await page.evaluate(() => window.devicePixelRatio);
  const encodedPng = await app.evaluate(async ({ BrowserWindow, screen }) => {
    const win = BrowserWindow.getAllWindows()[0];
    const image = await win.webContents.capturePage();
    return {
      png: image.toPNG().toString("base64"),
      scaleFactor: screen.getPrimaryDisplay().scaleFactor,
    };
  });

  fs.writeFileSync(shotPath, Buffer.from(encodedPng.png, "base64"));
  return { devicePixelRatio, scaleFactor: encodedPng.scaleFactor };
}

async function invokeDesktop(page, namespace, method, ...args) {
  return page.evaluate(
    async ({ namespace, method, args }) => window.kanvibeDesktop.invoke(namespace, method, args),
    { namespace, method, args },
  );
}

async function dismissDialogIfPresent(page) {
  try {
    await page.getByRole("button", { name: "닫기" }).first().click({ timeout: 2500 });
    await page.waitForTimeout(500);
  } catch {
    // 릴리스 노트 다이얼로그는 없을 수도 있다.
  }
}

async function main() {
  const fixturesRoot = path.join(OUT_DIR, "fixtures");
  const appDataDir = path.join(OUT_DIR, "app-data");
  fs.rmSync(fixturesRoot, { recursive: true, force: true });
  fs.rmSync(appDataDir, { recursive: true, force: true });
  fs.mkdirSync(fixturesRoot, { recursive: true });
  fs.mkdirSync(appDataDir, { recursive: true });

  const projectDirs = new Map();
  for (const fixture of PROJECT_FIXTURES) {
    projectDirs.set(fixture.name, createLocalRepository(fixturesRoot, fixture));
  }
  console.log(`[capture] created ${projectDirs.size} fixture repositories`);

  const { app, page } = await launchKanVibeElectron({
    rootDir: ROOT_DIR,
    appDataDir,
    viewport: BOARD_VIEWPORT,
    timeoutMs: 60000,
    actionTimeoutMs: 30000,
  });

  try {
    await page.waitForLoadState("domcontentloaded");
    await page.waitForTimeout(2500);
    await page.screenshot({ path: path.join(OUT_DIR, "00-launched.png") });
    await dismissDialogIfPresent(page);

    const projectIds = new Map();
    for (const fixture of PROJECT_FIXTURES) {
      const result = await invokeDesktop(page, "project", "registerProject", fixture.name, projectDirs.get(fixture.name));
      if (!result?.success || !result.project) {
        console.log(`[capture] register failed: ${fixture.name} -> ${result?.error}`);
        continue;
      }
      projectIds.set(fixture.name, result.project.id);
      console.log(`[capture] registered ${fixture.name} icon=${result.project.iconDataUrl ? "yes" : "no"} color=${result.project.color}`);
    }

    for (const task of TASK_FIXTURES) {
      const projectId = projectIds.get(task.project);
      if (!projectId) continue;

      const created = await invokeDesktop(page, "kanban", "createTask", {
        title: task.title,
        description: task.description ?? "",
        projectId,
      });
      if (task.status !== "todo" && created?.id) {
        await invokeDesktop(page, "kanban", "moveTaskToColumn", created.id, task.status, [created.id]);
      }
    }
    console.log(`[capture] seeded ${TASK_FIXTURES.length} tasks`);

    await page.reload();
    await page.waitForLoadState("domcontentloaded");
    await page.waitForTimeout(4000);
    await dismissDialogIfPresent(page);

    const shotPath = path.join(OUT_DIR, "board.png");
    const density = await captureRetinaScreenshot(app, page, shotPath);
    console.log(JSON.stringify({ ok: true, screenshot: shotPath, ...density }));
  } catch (error) {
    console.error("[capture] electron output:\n" + (typeof app.output === "function" ? app.output() : ""));
    throw error;
  } finally {
    await app.close().catch(() => {});
  }
}

main().catch((error) => {
  console.error("[capture] failed:", error);
  process.exit(1);
});
