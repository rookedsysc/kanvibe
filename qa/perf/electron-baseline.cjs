#!/usr/bin/env node
/* eslint-disable @typescript-eslint/no-require-imports */

const fs = require("node:fs");
const http = require("node:http");
const os = require("node:os");
const path = require("node:path");
const { spawn } = require("node:child_process");
const { performance } = require("node:perf_hooks");
const { chromium } = require("@playwright/test");
const { launchKanVibeElectron, resolveCdpPort } = require("../electron/lib/launchElectron.cjs");

const DEFAULT_ITERATIONS = 5;

function parseArgs(argv) {
  const args = {
    iterations: DEFAULT_ITERATIONS,
  };

  for (let index = 2; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--output") args.output = argv[++index];
    else if (arg === "--run-dir") args.runDir = argv[++index];
    else if (arg === "--iterations") args.iterations = Number.parseInt(argv[++index], 10);
    else if (arg === "--executable") args.executable = argv[++index];
  }

  if (!Number.isInteger(args.iterations) || args.iterations <= 0) {
    throw new Error(`Invalid --iterations value: ${args.iterations}`);
  }

  return args;
}

function timestampId() {
  return new Date().toISOString().replace(/[:.]/g, "-");
}

function percentile(values, fraction) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const index = Math.min(sorted.length - 1, Math.max(0, Math.ceil(sorted.length * fraction) - 1));
  return sorted[index];
}

function summarizeSamples(samples) {
  const values = samples.map((sample) => sample.ms);
  const total = values.reduce((sum, value) => sum + value, 0);

  return {
    count: values.length,
    avgMs: Number((total / values.length).toFixed(2)),
    minMs: Number(Math.min(...values).toFixed(2)),
    medianMs: Number(percentile(values, 0.5).toFixed(2)),
    p95Ms: Number(percentile(values, 0.95).toFixed(2)),
    maxMs: Number(Math.max(...values).toFixed(2)),
  };
}

function readProcessTable() {
  if (process.platform !== "linux") {
    return null;
  }

  const table = new Map();
  for (const entry of fs.readdirSync("/proc", { withFileTypes: true })) {
    if (!entry.isDirectory() || !/^\d+$/.test(entry.name)) continue;
    const pid = Number(entry.name);

    try {
      const stat = fs.readFileSync(path.join("/proc", entry.name, "stat"), "utf8");
      const afterCommand = stat.slice(stat.lastIndexOf(")") + 2).trim().split(/\s+/);
      const ppid = Number(afterCommand[1]);
      table.set(pid, { pid, ppid });
    } catch {
      // Processes can exit while /proc is being scanned.
    }
  }

  return table;
}

function readRssKb(pid) {
  try {
    const status = fs.readFileSync(path.join("/proc", String(pid), "status"), "utf8");
    const match = /^VmRSS:\s+(\d+)\s+kB$/m.exec(status);
    return match ? Number(match[1]) : 0;
  } catch {
    return 0;
  }
}

function sampleProcessTreeMemory(rootPid) {
  const table = readProcessTable();
  if (!table) {
    return {
      supported: false,
      reason: "process-tree RSS sampling is implemented for Linux /proc only",
    };
  }

  const childrenByParent = new Map();
  for (const processInfo of table.values()) {
    if (!childrenByParent.has(processInfo.ppid)) childrenByParent.set(processInfo.ppid, []);
    childrenByParent.get(processInfo.ppid).push(processInfo.pid);
  }

  const pids = [];
  const queue = [rootPid];
  const seen = new Set();
  while (queue.length) {
    const pid = queue.shift();
    if (seen.has(pid)) continue;
    seen.add(pid);
    if (!table.has(pid)) continue;
    pids.push(pid);
    for (const childPid of childrenByParent.get(pid) || []) {
      queue.push(childPid);
    }
  }

  const totalRssKb = pids.reduce((sum, pid) => sum + readRssKb(pid), 0);

  return {
    supported: true,
    rootPid,
    processCount: pids.length,
    pids,
    totalRssKb,
    totalRssMb: Number((totalRssKb / 1024).toFixed(2)),
  };
}

function collectProcessOutput(child) {
  const output = [];
  child.stdout?.on("data", (chunk) => output.push(chunk.toString("utf8")));
  child.stderr?.on("data", (chunk) => output.push(chunk.toString("utf8")));
  return () => output.join("");
}

function wait(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function requestJson(url) {
  return new Promise((resolve, reject) => {
    const request = http.get(url, (response) => {
      const chunks = [];
      response.on("data", (chunk) => chunks.push(chunk));
      response.on("end", () => {
        if (!response.statusCode || response.statusCode < 200 || response.statusCode >= 300) {
          reject(new Error(`GET ${url} returned ${response.statusCode}`));
          return;
        }

        try {
          resolve(JSON.parse(Buffer.concat(chunks).toString("utf8")));
        } catch (error) {
          reject(error);
        }
      });
    });
    request.on("error", reject);
    request.setTimeout(1000, () => {
      request.destroy(new Error(`GET ${url} timed out`));
    });
  });
}

async function waitForCdpEndpoint(cdpPort, child, output, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (child.exitCode !== null || child.signalCode) {
      throw new Error(`Packaged Electron exited before CDP became available: exit=${child.exitCode} signal=${child.signalCode}\n${output()}`);
    }

    try {
      return await requestJson(`http://127.0.0.1:${cdpPort}/json/version`);
    } catch {
      await wait(100);
    }
  }

  throw new Error(`Timed out waiting for packaged Electron CDP on port ${cdpPort}\n${output()}`);
}

async function waitForFirstPage(browser, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    for (const context of browser.contexts()) {
      const page = context.pages()[0];
      if (page) return page;
    }
    await wait(100);
  }

  throw new Error("Timed out waiting for packaged Electron first page");
}

function waitForChildExit(child, timeoutMs = 2000) {
  if (child.exitCode !== null || child.signalCode) return Promise.resolve();

  return new Promise((resolve) => {
    const timeout = setTimeout(resolve, timeoutMs);
    child.once("exit", () => {
      clearTimeout(timeout);
      resolve();
    });
  });
}

async function launchPackagedElectron(options) {
  const cdpPort = await resolveCdpPort(options);
  const child = spawn(options.executablePath, [
    `--remote-debugging-port=${cdpPort}`,
    "--no-sandbox",
  ], {
    cwd: options.rootDir,
    env: {
      ...process.env,
      CI: process.env.CI || "1",
      KANVIBE_QA_MODE: "1",
      KANVIBE_QA_OUTPUT_DIR: options.outputDir || "",
      KANVIBE_APP_DATA_DIR: options.appDataDir || process.env.KANVIBE_APP_DATA_DIR || "",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  const output = collectProcessOutput(child);
  let browser;

  try {
    await waitForCdpEndpoint(cdpPort, child, output, options.timeoutMs || 60_000);
    browser = await chromium.connectOverCDP(`http://127.0.0.1:${cdpPort}`);
    const page = await waitForFirstPage(browser, options.timeoutMs || 60_000);
    page.setDefaultTimeout(options.actionTimeoutMs || 10_000);
    page.setDefaultNavigationTimeout(options.navigationTimeoutMs || 30_000);
    try {
      await page.setViewportSize(options.viewport || { width: 1440, height: 960 });
    } catch {
      // Packaged Electron windows can reject viewport updates before first paint.
    }

    return {
      app: {
        process: () => child,
        output,
        async close() {
          await browser?.close().catch(() => {});
          if (child.exitCode === null && !child.signalCode) {
            child.kill();
          }
          await waitForChildExit(child);
        },
      },
      page,
      cdpPort,
    };
  } catch (error) {
    await browser?.close().catch(() => {});
    if (child.exitCode === null && !child.signalCode) child.kill();
    await waitForChildExit(child);
    throw error;
  }
}

async function launchPerfElectron(options) {
  if (options.executablePath) {
    return launchPackagedElectron(options);
  }

  return launchKanVibeElectron(options);
}

async function invokeDesktop(page, namespace, method, ...args) {
  return page.evaluate(
    async ({ namespace, method, args }) => window.kanvibeDesktop.invoke(namespace, method, args),
    { namespace, method, args },
  );
}

async function timedOperation(samples, name, fn) {
  const startedAt = performance.now();
  const value = await fn();
  samples.push({ name, ms: Number((performance.now() - startedAt).toFixed(2)) });
  return value;
}

async function waitForBoardReady(page) {
  await page.waitForFunction(() => window.kanvibeDesktop?.isDesktop === true, null, { timeout: 30_000 });
  await page.waitForFunction(
    () => !document.querySelector("[data-testid='board-route-skeleton']")
      && document.querySelectorAll("[data-rfd-droppable-id]").length >= 5,
    null,
    { timeout: 30_000 },
  );
}

async function run() {
  const args = parseArgs(process.argv);
  const rootDir = path.resolve(__dirname, "..", "..");
  const runDir = path.resolve(args.runDir || path.join(rootDir, "qa", "perf-output", `electron-baseline-${timestampId()}`));
  const appDataDir = path.join(runDir, "app-data");
  const packagedExecutable = path.join(rootDir, "dist", "linux-unpacked", "kanvibe");
  const executablePath = args.executable
    ? path.resolve(args.executable)
    : fs.existsSync(packagedExecutable)
      ? packagedExecutable
      : null;
  fs.mkdirSync(runDir, { recursive: true });

  const launchStartedAt = performance.now();
  const { app, page } = await launchPerfElectron({
    rootDir,
    ...(executablePath ? { executablePath } : {}),
    appDataDir,
    outputDir: runDir,
    timeoutMs: 60_000,
    actionTimeoutMs: 15_000,
    viewport: { width: 1440, height: 960 },
  });

  const electronPid = app.process().pid;
  const firstWindowMs = Number((performance.now() - launchStartedAt).toFixed(2));
  let result;

  try {
    await waitForBoardReady(page);
    const boardReadyMs = Number((performance.now() - launchStartedAt).toFixed(2));
    const memoryAfterBoardReady = sampleProcessTreeMemory(electronPid);
    const samples = [];

    await timedOperation(samples, "kanban.getTasksByStatus.initial", () => invokeDesktop(page, "kanban", "getTasksByStatus"));

    for (let index = 0; index < args.iterations; index += 1) {
      const title = `Perf baseline task ${timestampId()} ${index + 1}`;
      const created = await timedOperation(samples, "kanban.createTask.noWorktree", () => invokeDesktop(page, "kanban", "createTask", {
        title,
        description: "Created by qa/perf/electron-baseline.cjs",
        priority: "medium",
      }));

      await timedOperation(samples, "kanban.updateTaskStatus.progress", () => invokeDesktop(page, "kanban", "updateTaskStatus", created.id, "progress"));
      await timedOperation(samples, "kanban.getTasksByStatus.afterUpdate", () => invokeDesktop(page, "kanban", "getTasksByStatus"));
      await timedOperation(samples, "kanban.deleteTask.noWorktree", () => invokeDesktop(page, "kanban", "deleteTask", created.id));
    }

    const samplesByName = samples.reduce((acc, sample) => {
      if (!acc[sample.name]) acc[sample.name] = [];
      acc[sample.name].push(sample);
      return acc;
    }, {});

    result = {
      status: "PASS",
      timestamp: new Date().toISOString(),
      platform: {
        node: process.version,
        os: `${os.type()} ${os.release()} ${os.arch()}`,
        display: process.env.DISPLAY || null,
        waylandDisplay: process.env.WAYLAND_DISPLAY || null,
      },
      runtime: {
        kind: executablePath ? "packaged" : "source-electron-package",
        executablePath,
      },
      launch: {
        firstWindowMs,
        boardReadyMs,
      },
      memory: {
        afterBoardReady: memoryAfterBoardReady,
        afterOperations: sampleProcessTreeMemory(electronPid),
      },
      operations: {
        iterations: args.iterations,
        summaries: Object.fromEntries(
          Object.entries(samplesByName).map(([name, nameSamples]) => [name, summarizeSamples(nameSamples)]),
        ),
        samples,
      },
      runDir,
    };
  } finally {
    await app.close().catch(() => {});
  }

  const outputPath = args.output ? path.resolve(args.output) : path.join(runDir, "electron-baseline.json");
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
  console.log(JSON.stringify({ outputPath, ...result }, null, 2));
}

run().catch((error) => {
  console.error(error);
  process.exit(1);
});
