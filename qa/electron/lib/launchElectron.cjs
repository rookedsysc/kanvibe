const { chromium } = require("@playwright/test");
const electronExecutable = require("electron");
const http = require("node:http");
const { spawn } = require("node:child_process");

function waitForCdp(port, timeoutMs = 45000) {
  const deadline = Date.now() + timeoutMs;
  const url = `http://127.0.0.1:${port}/json/version`;

  return new Promise((resolve, reject) => {
    const attempt = () => {
      const request = http.get(url, (response) => {
        response.resume();
        if (response.statusCode && response.statusCode < 500) {
          resolve(url);
          return;
        }
        retry();
      });

      request.on("error", retry);
      request.setTimeout(1000, () => {
        request.destroy();
        retry();
      });
    };

    const retry = () => {
      if (Date.now() > deadline) {
        reject(new Error(`Electron CDP endpoint did not become ready on ${url}`));
        return;
      }
      setTimeout(attempt, 250);
    };

    attempt();
  });
}

async function launchKanVibeElectron(options = {}) {
  const rootDir = options.rootDir || process.cwd();
  const cdpPort = Number.parseInt(process.env.KANVIBE_QA_CDP_PORT || "19337", 10);
  const child = spawn(electronExecutable, [
    `--remote-debugging-port=${cdpPort}`,
    "--no-sandbox",
    rootDir,
  ], {
    cwd: rootDir,
    env: {
      ...process.env,
      CI: process.env.CI || "1",
      KANVIBE_QA_MODE: "1",
      KANVIBE_QA_OUTPUT_DIR: options.outputDir || "",
      KANVIBE_APP_DATA_DIR: options.appDataDir || process.env.KANVIBE_APP_DATA_DIR || "",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  const output = [];
  child.stdout.on("data", (chunk) => output.push(chunk.toString("utf8")));
  child.stderr.on("data", (chunk) => output.push(chunk.toString("utf8")));

  let browser;
  try {
    await waitForCdp(cdpPort, options.timeoutMs || 45000);
    browser = await chromium.connectOverCDP(`http://127.0.0.1:${cdpPort}`);
    const context = browser.contexts()[0] || await browser.newContext();
    let page = context.pages().find((candidate) => !candidate.url().startsWith("devtools://"));
    if (!page) {
      page = await context.waitForEvent("page", { timeout: options.timeoutMs || 45000 });
    }

    page.setDefaultTimeout(options.actionTimeoutMs || 10000);
    page.setDefaultNavigationTimeout(options.navigationTimeoutMs || 30000);
    try {
      await page.setViewportSize(options.viewport || { width: 1440, height: 960 });
    } catch {
      // Electron windows can refuse viewport changes before first paint; non-fatal.
    }

    const app = {
      process: () => child,
      async close() {
        if (browser) await browser.close().catch(() => {});
        if (!child.killed) child.kill("SIGTERM");
      },
      output: () => output.join(""),
    };

    return { app, page };
  } catch (error) {
    if (browser) await browser.close().catch(() => {});
    if (!child.killed) child.kill("SIGTERM");
    const detail = output.join("").trim();
    if (detail && error instanceof Error) {
      error.message = `${error.message}\nElectron output:\n${detail}`;
    }
    throw error;
  }
}

module.exports = { launchKanVibeElectron };
