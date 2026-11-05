const fs = require("node:fs");
const path = require("node:path");
const { spawn } = require("node:child_process");

async function launchKanVibeTauri(options = {}) {
  const rootDir = options.rootDir || process.cwd();
  const appDataDir = path.resolve(options.appDataDir);
  fs.mkdirSync(appDataDir, { recursive: true });
  const metadataPath = path.join(appDataDir, "qa-bridge.json");
  fs.rmSync(metadataPath, { force: true });
  const child = spawn(options.executablePath || path.join(rootDir, "src-tauri/target/debug/kanvibe"), [], {
    cwd: rootDir,
    env: { ...process.env, KANVIBE_QA_MODE: "1", KANVIBE_APP_DATA_DIR: appDataDir, KANVIBE_DB_PATH: path.join(appDataDir, "kanvibe.db"), KANVIBE_HOOK_PORT: String(options.hookPort || 19736) },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let output = "";
  child.stdout.on("data", chunk => { output += chunk; });
  child.stderr.on("data", chunk => { output += chunk; });
  const deadline = Date.now() + (options.timeoutMs || 45000);
  let metadata;
  const pause = () => new Promise(resolve => setTimeout(resolve, 100));
  try {
    while (Date.now() < deadline) {
      if (child.exitCode !== null) throw new Error(`Tauri exited: ${child.exitCode}\n${output}`);
      try { metadata = JSON.parse(fs.readFileSync(metadataPath, "utf8")); break; } catch { await pause(); }
    }
    if (!metadata) throw new Error(`Tauri QA bridge did not start\n${output}`);
    const evaluate = async (script) => {
      const response = await fetch(`http://127.0.0.1:${metadata.port}/eval`, {
        method: "POST", headers: { Authorization: `Bearer ${metadata.token}`, "Content-Type": "application/json" },
        body: JSON.stringify({ script }), signal: AbortSignal.timeout(50000),
      });
      const result = await response.json();
      if (!response.ok) throw new Error(result.error || `QA HTTP ${response.status}`);
      return result.value;
    };
    const page = {
      evaluate: (fn, arg) => evaluate(`return (${fn.toString()})(${JSON.stringify(arg) ?? "undefined"});`),
      async waitForFunction(fn, arg) {
        const until = Date.now() + 30000;
        while (Date.now() < until) { if (await page.evaluate(fn, arg)) return; await pause(); }
        throw new Error(`WebView condition timed out: ${fn}`);
      },
      async screenshot({ path: outputPath }) {
        const png = await evaluate("return await window.__TAURI_INTERNALS__.invoke('qa_snapshot');");
        fs.writeFileSync(outputPath, Buffer.from(png, "base64"));
      },
      async exposeFunction(name, fn) {
        if (name !== "__captureNotificationQaChange") throw new Error(`Unsupported native QA callback: ${name}`);
        await evaluate(`window[${JSON.stringify(name)}]=()=>{window.__notificationQaChangeCount=(window.__notificationQaChangeCount||0)+1;};return true;`);
        const timer = setInterval(() => {
          void evaluate("const n=window.__notificationQaChangeCount||0;window.__notificationQaChangeCount=0;return n;")
            .then(n => { for (let i = 0; i < n; i++) fn(); }).catch(() => {});
        }, 100);
        child.once("exit", () => clearInterval(timer));
      },
      async addInitScript(fn) { await page.evaluate(fn); },
    };
    await page.waitForFunction(() => Boolean(window.kanvibeDesktop));
    return {
      page,
      app: {
        output: () => output,
        process: () => child,
        async close() {
          if (child.exitCode !== null) return;
          await new Promise(resolve => { child.once("exit", resolve); child.kill("SIGTERM"); });
        },
      },
    };
  } catch (error) { child.kill("SIGTERM"); throw error; }
}
module.exports = { launchKanVibeTauri };
