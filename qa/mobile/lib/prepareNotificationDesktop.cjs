#!/usr/bin/env node
/* eslint-disable @typescript-eslint/no-require-imports */
const fs = require("node:fs");
const http = require("node:http");
const net = require("node:net");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const { launchKanVibeTauri } = require("../../tauri/lib/launchTauri.cjs");
function createLocalShellEnvironment() {
  return Object.fromEntries(Object.entries(process.env).filter(([key]) => !["PORT", "HOST", "NODE_ENV"].includes(key) && !key.startsWith("KANVIBE_")));
}

const ROOT_DIR = path.resolve(__dirname, "../../..");
const DESKTOP_API = "http://127.0.0.1:19736";
const PROBE_PORT = 19737;
const PROBE_URL = `http://127.0.0.1:${PROBE_PORT}`;
const LATEST_TASK_TITLE = "Maestro Open Task Latest";
const LIVE_NOTIFICATION_TITLE = "Maestro Live Notice";
let desktopNotificationEventCount = 0;

// 기존 terminal fixture와 달리 실제 renderer IPC로 알림을 읽고 바꾸며, 사용자 DB를 열지 않는다.
function git(repoPath, ...args) {
  return execFileSync("direnv", ["exec", ROOT_DIR, "git", "-C", repoPath, ...args], {
    cwd: ROOT_DIR,
    env: createLocalShellEnvironment(),
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

function createRepository(runDir, name) {
  const repoPath = path.join(runDir, "repositories", name);
  fs.mkdirSync(repoPath, { recursive: true });
  git(repoPath, "init", "-b", "main");
  git(repoPath, "config", "user.email", "qa@kanvibe.local");
  git(repoPath, "config", "user.name", "KanVibe QA");
  fs.writeFileSync(path.join(repoPath, "README.md"), `# ${name}\n`);
  git(repoPath, "add", "README.md");
  git(repoPath, "commit", "-m", "chore: isolated mobile notification fixture");
  return repoPath;
}

function availablePort(port = 0) {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once("error", reject);
    server.listen(port, "127.0.0.1", () => {
      const selectedPort = server.address().port;
      server.close((error) => error ? reject(error) : resolve(selectedPort));
    });
  });
}

async function waitForUrl(url, child) {
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    if (child?.exitCode !== null && child?.exitCode !== undefined) {
      throw new Error(`Tauri exited before readiness: ${child.exitCode}`);
    }
    try {
      const response = await fetch(url, { signal: AbortSignal.timeout(1000) });
      if (response.ok) return;
    } catch {
      // 자신의 서버가 준비될 때까지만 기다린다.
    }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(`Fixture server did not become ready: ${url}`);
}

function invokeDesktop(page, namespace, method, ...args) {
  return page.evaluate(({ namespace, method, args }) => window.kanvibeDesktop.invoke(namespace, method, args), {
    namespace, method, args,
  });
}

async function createProject(page, name, repoPath, rootTitle) {
  const result = await invokeDesktop(page, "project", "registerProject", name, repoPath);
  if (!result.success || !result.project) throw new Error(`Project fixture failed: ${JSON.stringify(result)}`);
  const board = await invokeDesktop(page, "kanban", "getTasksByStatus");
  const root = Object.values(board.tasks).flat().find((task) => task.projectId === result.project.id && task.branchName === "main");
  if (!root) throw new Error(`Project root task missing: ${name}`);
  await invokeDesktop(page, "kanban", "updateTask", root.id, { title: rootTitle });
  return { ...result.project, rootTaskId: root.id };
}

async function createNotification(page, title, taskId) {
  await page.evaluate((payload) => window.kanvibeDesktop.showNotification(payload), {
    title,
    body: `${title} fixture body`,
    locale: "ko",
    ...(taskId ? { taskId } : {}),
    dedupeKey: `mobile-filter-notifications:${title}`,
  });
  const notifications = await page.evaluate(() => window.kanvibeDesktop.listNotifications());
  const notification = notifications.find((entry) => entry.title === title);
  if (!notification) throw new Error(`Notification fixture missing: ${title}`);
  return notification;
}

async function seedDesktop(page, runDir) {
  await invokeDesktop(page, "appSettings", "setBackgroundSyncEnabled", false);
  // 필터/알림 검증에는 pane이 필요 없으므로 fixture가 전역 tmux 세션을 만들지 않게 한다.
  await invokeDesktop(page, "appSettings", "setDefaultSessionType", "terminal");
  const alpha = await createProject(page, "Maestro Alpha", createRepository(runDir, "maestro-alpha"), "Maestro Alpha Root");
  const beta = await createProject(page, "Maestro Beta", createRepository(runDir, "maestro-beta"), "Maestro Beta Root");
  const worktree = await invokeDesktop(page, "kanban", "createTask", {
    title: "Maestro Alpha Worktree",
    branchName: "qa/mobile-filter",
    sessionType: "terminal",
    projectId: alpha.id,
  });
  const target = await invokeDesktop(page, "kanban", "createTask", {
    title: "Maestro Open Task Original",
    projectId: beta.id,
  });
  if (!worktree.worktreePath || !fs.existsSync(worktree.worktreePath)) throw new Error("Actual fixture worktree was not created");
  const notifications = {};
  for (const [key, title, taskId] of [
    ["mobileRead", "Maestro Mobile Read", null],
    ["desktopRead", "Maestro Desktop Read", null],
    ["openTask", "Maestro Open Latest Task", target.id],
    ["alreadyRead", "Maestro Already Read", null],
  ]) {
    notifications[key] = await createNotification(page, title, taskId);
  }
  notifications.alreadyRead = await page.evaluate((id) => window.kanvibeDesktop.markNotificationRead(id), notifications.alreadyRead.id);
  await page.exposeFunction("__captureNotificationQaChange", () => { desktopNotificationEventCount += 1; });
  // WebView가 다시 로드돼도 실제 IPC 수신 기록은 fixture 프로세스에 남긴다.
  await page.addInitScript(() => {
    window.addEventListener("DOMContentLoaded", () => {
      window.kanvibeDesktop.onNotificationsChanged(() => { void window.__captureNotificationQaChange(); });
    }, { once: true });
  });
  await page.evaluate(() => {
    window.kanvibeDesktop.onNotificationsChanged(() => { void window.__captureNotificationQaChange(); });
  });
  return {
    pid: process.pid,
    desktopApi: DESKTOP_API,
    probeUrl: PROBE_URL,
    projects: { alpha, beta },
    tasks: { alphaRootId: alpha.rootTaskId, betaRootId: beta.rootTaskId, worktreeId: worktree.id, targetId: target.id },
    notifications,
    latestTaskTitle: LATEST_TASK_TITLE,
    liveNotificationTitle: LIVE_NOTIFICATION_TITLE,
    worktreePath: worktree.worktreePath,
  };
}

async function readDesktopState(page, fixture) {
  const state = await page.evaluate(async () => {
    const [notifications, board] = await Promise.all([
      window.kanvibeDesktop.listNotifications(),
      window.kanvibeDesktop.invoke("mobileBridge", "getMobileBoard", []),
    ]);
    return { notifications, board };
  });
  return { ...state, fixture, desktopEventCount: desktopNotificationEventCount, unreadCount: state.notifications.filter((entry) => !entry.isRead).length };
}

async function waitForState(page, fixture, requestUrl) {
  const unread = requestUrl.searchParams.get("unread");
  const eventsAfter = requestUrl.searchParams.get("eventsAfter");
  const deadline = Date.now() + 8000;
  let state;
  do {
    state = await readDesktopState(page, fixture);
    if ((unread === null || state.unreadCount === Number(unread))
      && (eventsAfter === null || state.desktopEventCount > Number(eventsAfter))) return state;
    await new Promise((resolve) => setTimeout(resolve, 100));
  } while (Date.now() < deadline);
  return state;
}

async function startProbe(page, fixture, runDir) {
  const actions = {
    "/desktop/read-one": () => page.evaluate((id) => window.kanvibeDesktop.markNotificationRead(id), fixture.notifications.desktopRead.id),
    "/desktop/read-all": () => page.evaluate(() => window.kanvibeDesktop.markAllNotificationsRead()),
    "/desktop/create": () => createNotification(page, LIVE_NOTIFICATION_TITLE, null),
    "/desktop/update-task": () => invokeDesktop(page, "kanban", "updateTask", fixture.tasks.targetId, { title: LATEST_TASK_TITLE }),
  };
  const server = http.createServer(async (request, response) => {
    const requestUrl = new URL(request.url, PROBE_URL);
    const action = request.method === "POST" ? actions[requestUrl.pathname] : null;
    if (!(request.method === "GET" && requestUrl.pathname === "/state") && !action) {
      response.writeHead(404).end();
      return;
    }
    try {
      if (action) await action();
      const state = await waitForState(page, fixture, requestUrl);
      fs.appendFileSync(path.join(runDir, "observations.jsonl"), `${JSON.stringify({ time: new Date().toISOString(), action: requestUrl.pathname, ...state })}\n`);
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify(state));
    } catch (error) {
      response.writeHead(500, { "Content-Type": "application/json" });
      response.end(JSON.stringify({ error: error.message }));
    }
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(PROBE_PORT, "127.0.0.1", resolve);
  });
  return server;
}

async function main() {
  const runDirIndex = process.argv.indexOf("--run-dir");
  if (runDirIndex === -1 || !process.argv[runDirIndex + 1]) throw new Error("--run-dir <fresh directory> is required");
  const runDir = path.resolve(process.argv[runDirIndex + 1]);
  const appDataDir = path.join(runDir, "app-data");
  if (fs.existsSync(appDataDir)) throw new Error("Use a fresh --run-dir to preserve earlier QA evidence and reset fixture state");
  await availablePort(19736);
  await availablePort(PROBE_PORT);
  fs.mkdirSync(appDataDir, { recursive: true });
  let app;
  let probe;
  let stopping;
  const stop = () => {
    stopping ??= (async () => {
      probe?.closeAllConnections();
      probe?.close();
      if (app) {
        fs.writeFileSync(path.join(runDir, "desktop.log"), app.output());
        await app.close();
      }
    })();
    return stopping;
  };
  process.once("SIGTERM", () => { void stop().finally(() => process.exit(0)); });
  process.once("SIGINT", () => { void stop().finally(() => process.exit(0)); });
  try {
    const launched = await launchKanVibeTauri({ rootDir: ROOT_DIR, appDataDir, hookPort: 19736, timeoutMs: 60_000 });
    app = launched.app;
    const page = launched.page;
    await page.waitForFunction(() => Boolean(window.kanvibeDesktop?.listNotifications));
    await waitForUrl(`${DESKTOP_API}/api/hooks/health`, app.process());
    const fixture = { ...await seedDesktop(page, runDir), appDataDir, runtime: "tauri" };
    fs.writeFileSync(path.join(runDir, "fixture.json"), `${JSON.stringify(fixture, null, 2)}\n`);
    await page.screenshot({ path: path.join(runDir, "desktop-fixture.png") });
    probe = await startProbe(page, fixture, runDir);
    console.log(JSON.stringify(fixture));
  } catch (error) {
    await stop();
    throw error;
  }
}

if (require.main === module) main().catch((error) => { console.error(error.stack); process.exit(1); });
