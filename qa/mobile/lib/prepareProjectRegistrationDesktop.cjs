#!/usr/bin/env node
const fs = require("node:fs");
const http = require("node:http");
const net = require("node:net");
const path = require("node:path");
const { execFileSync, spawn } = require("node:child_process");
const { launchKanVibeElectron } = require("../../electron/lib/launchElectron.cjs");
const { createLocalShellEnvironment } = require("../../../src/lib/shellEnvironment.ts");

const ROOT_DIR = path.resolve(__dirname, "../../..");
const HOOK_PORT = 20736;
const MOBILE_PORT = 20746;
const PROBE_PORT = 20737;
const PROJECT_NAME = "maestro-mobile-register";

function availablePort(port = 0) {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once("error", reject);
    server.listen(port, "127.0.0.1", () => {
      const selected = server.address().port;
      server.close((error) => error ? reject(error) : resolve(selected));
    });
  });
}

async function waitForUrl(url, child) {
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    if (child && child.exitCode !== null) throw new Error(`Fixture process exited: ${child.exitCode}`);
    try {
      if ((await fetch(url, { signal: AbortSignal.timeout(1000) })).ok) return;
    } catch {
      // 준비 중인 이 실행의 서버만 기다린다.
    }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(`Fixture readiness timed out: ${url}`);
}

function createRepository(runDir) {
  const repoPath = path.join(runDir, "repositories", PROJECT_NAME);
  fs.mkdirSync(repoPath, { recursive: true });
  const git = (...args) => execFileSync("direnv", ["exec", ROOT_DIR, "git", "-C", repoPath, ...args], {
    env: createLocalShellEnvironment(), stdio: "pipe",
  });
  git("init", "-b", "main");
  git("config", "user.email", "qa@kanvibe.local");
  git("config", "user.name", "KanVibe QA");
  fs.writeFileSync(path.join(repoPath, "README.md"), "# Mobile registration fixture\n");
  git("add", "README.md");
  git("commit", "-m", "chore: isolated mobile registration fixture");
  return repoPath;
}

async function main() {
  const index = process.argv.indexOf("--run-dir");
  if (index < 0 || !process.argv[index + 1]) throw new Error("--run-dir <fresh directory> is required");
  const runDir = path.resolve(process.argv[index + 1]);
  const appDataDir = path.join(runDir, "app-data");
  if (fs.existsSync(appDataDir)) throw new Error("Use a fresh run directory");
  await Promise.all([HOOK_PORT, MOBILE_PORT, PROBE_PORT].map(availablePort));
  fs.mkdirSync(appDataDir, { recursive: true });
  const rendererPort = await availablePort();
  const rendererUrl = `http://127.0.0.1:${rendererPort}`;
  const viteLog = fs.openSync(path.join(runDir, "vite.log"), "a");
  const vite = spawn("pnpm", ["exec", "vite", "--host", "127.0.0.1", "--port", String(rendererPort), "--strictPort"], {
    cwd: ROOT_DIR, env: createLocalShellEnvironment(), stdio: ["ignore", viteLog, viteLog], detached: true,
  });
  fs.closeSync(viteLog);
  let app;
  let probe;
  let stopping = false;
  const stop = async () => {
    if (stopping) return;
    stopping = true;
    probe?.closeAllConnections();
    probe?.close();
    if (app) {
      await app.evaluate(() => globalThis.__projectRegistrationQaServer?.close()).catch(() => {});
      fs.writeFileSync(path.join(runDir, "desktop.log"), app.output());
      await app.close();
    }
    try { process.kill(-vite.pid, "SIGTERM"); } catch { /* 이 실행의 자식이 이미 종료됐다. */ }
  };
  process.once("SIGTERM", () => { void stop().finally(() => process.exit(0)); });
  process.once("SIGINT", () => { void stop().finally(() => process.exit(0)); });
  try {
    await waitForUrl(rendererUrl, vite);
    const launched = await launchKanVibeElectron({ rootDir: ROOT_DIR, rendererUrl, outputDir: runDir, appDataDir, timeoutMs: 60_000 });
    app = launched.app;
    const page = launched.page;
    await page.waitForFunction(() => Boolean(window.kanvibeDesktop?.invoke));
    const identity = await app.evaluate(async ({ app: electronApp }, config) => {
      if (electronApp.getPath("userData") !== config.appDataDir) throw new Error("App-data ownership mismatch");
      const { createRequire } = process.getBuiltinModule("module");
      const loadModule = createRequire(`${config.rootDir}/package.json`);
      const { setHookServerPort } = loadModule(`${config.rootDir}/src/lib/hookEndpoint.ts`);
      const { createHookServer } = loadModule(`${config.rootDir}/electron/hookServer.js`);
      setHookServerPort(config.hookPort);
      globalThis.__projectRegistrationQaServer = createHookServer({
        host: "127.0.0.1", port: config.hookPort, mobileTlsPort: config.mobilePort,
        mobileTlsDirectory: `${config.appDataDir}/mobile-tls`, isDevPairingRouteEnabled: true,
      });
      return { pid: process.pid, appDataDir: electronApp.getPath("userData") };
    }, { rootDir: ROOT_DIR, appDataDir, hookPort: HOOK_PORT, mobilePort: MOBILE_PORT });
    await waitForUrl(`http://127.0.0.1:${HOOK_PORT}/api/hooks/health`, app.process());
    for (const port of [HOOK_PORT, MOBILE_PORT]) {
      const deadline = Date.now() + 15_000;
      let owners = [];
      while (Date.now() < deadline) {
        try {
          owners = execFileSync("lsof", ["-t", `-iTCP:${port}`, "-sTCP:LISTEN"], { encoding: "utf8" }).trim().split("\n");
          if (owners.length === 1 && Number(owners[0]) === identity.pid) break;
        } catch { /* 인증서가 만들어지면 TLS 리스너가 열린다. */ }
        await new Promise((resolve) => setTimeout(resolve, 200));
      }
      if (owners.length !== 1 || Number(owners[0]) !== identity.pid) throw new Error(`Listener ownership mismatch: ${port} ${owners}`);
    }
    const invoke = (namespace, method, ...args) => page.evaluate(({ namespace, method, args }) => window.kanvibeDesktop.invoke(namespace, method, args), { namespace, method, args });
    await invoke("appSettings", "setBackgroundSyncEnabled", false);
    await invoke("appSettings", "setDefaultSessionType", "terminal");
    const repoPath = createRepository(runDir);
    const fixture = { ...identity, repoPath, browsePath: path.dirname(repoPath), projectName: PROJECT_NAME, hookPort: HOOK_PORT, mobilePort: MOBILE_PORT };
    fs.writeFileSync(path.join(runDir, "fixture.json"), `${JSON.stringify(fixture, null, 2)}\n`);
    probe = http.createServer(async (request, response) => {
      if (request.method !== "GET" || request.url !== "/state") return response.writeHead(404).end();
      try {
        const board = await invoke("mobileBridge", "getMobileBoard");
        const state = { fixture, board };
        fs.appendFileSync(path.join(runDir, "observations.jsonl"), `${JSON.stringify(state)}\n`);
        response.writeHead(200, { "Content-Type": "application/json" }).end(JSON.stringify(state));
      } catch (error) {
        response.writeHead(500).end(JSON.stringify({ error: error.message }));
      }
    });
    await new Promise((resolve, reject) => {
      probe.once("error", reject);
      probe.listen(PROBE_PORT, "127.0.0.1", resolve);
    });
    console.log(JSON.stringify(fixture));
  } catch (error) {
    await stop();
    throw error;
  }
}

main().catch((error) => { console.error(error.stack); process.exit(1); });
