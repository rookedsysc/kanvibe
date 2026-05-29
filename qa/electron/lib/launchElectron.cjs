const { _electron: electron } = require("@playwright/test");
const electronExecutable = require("electron");

function collectProcessOutput(child) {
  const output = [];
  child.stdout?.on("data", (chunk) => output.push(chunk.toString("utf8")));
  child.stderr?.on("data", (chunk) => output.push(chunk.toString("utf8")));
  return () => output.join("");
}

async function launchKanVibeElectron(options = {}) {
  const rootDir = options.rootDir || process.cwd();
  const cdpPort = Number.parseInt(process.env.KANVIBE_QA_CDP_PORT || "19337", 10);
  let app;

  try {
    app = await electron.launch({
      executablePath: electronExecutable,
      args: [
        `--remote-debugging-port=${cdpPort}`,
        "--no-sandbox",
        rootDir,
      ],
      cwd: rootDir,
      env: {
        ...process.env,
        CI: process.env.CI || "1",
        KANVIBE_QA_MODE: "1",
        KANVIBE_QA_OUTPUT_DIR: options.outputDir || "",
        KANVIBE_APP_DATA_DIR: options.appDataDir || process.env.KANVIBE_APP_DATA_DIR || "",
      },
      timeout: options.timeoutMs || 45000,
    });

    const child = app.process();
    const output = collectProcessOutput(child);
    const page = await app.firstWindow({ timeout: options.timeoutMs || 45000 });

    page.setDefaultTimeout(options.actionTimeoutMs || 10000);
    page.setDefaultNavigationTimeout(options.navigationTimeoutMs || 30000);
    try {
      await page.setViewportSize(options.viewport || { width: 1440, height: 960 });
    } catch {
      // Electron windows can refuse viewport changes before first paint; non-fatal.
    }

    const originalClose = app.close.bind(app);
    app.close = async () => {
      await originalClose().catch(() => {});
    };
    app.output = output;

    return { app, page, cdpPort };
  } catch (error) {
    if (app) await app.close().catch(() => {});
    throw error;
  }
}

module.exports = { launchKanVibeElectron };
