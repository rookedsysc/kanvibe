#!/usr/bin/env node

const { execFileSync } = require("node:child_process");

const REQUIRED_NODE_MAJOR = 24;
const isElectronRuntime = Boolean(process.versions.electron) || process.argv.includes("--electron");
const hasAttemptedRebuild = process.env.KANVIBE_NATIVE_REBUILD_ATTEMPTED === "1";

function isPackagedElectronRuntime() {
  return isElectronRuntime && !process.defaultApp;
}

function getNodeMajor() {
  return Number.parseInt(process.versions.node.split(".")[0] || "0", 10);
}

function isNativeModuleMismatch(error) {
  const message = String(error?.stack || error?.message || error || "");
  return (
    message.includes("NODE_MODULE_VERSION") ||
    message.includes("did not self-register") ||
    message.includes("ERR_DLOPEN_FAILED") ||
    message.includes("compiled against a different Node.js version")
  );
}

function printNodeVersionGuidance() {
  const currentVersion = process.versions.node;
  console.error(
    `[kanvibe] Unsupported Node.js runtime ${currentVersion}. KanVibe requires Node ${REQUIRED_NODE_MAJOR}.x for local scripts.`,
  );
  console.error(`[kanvibe] Run \`nvm use ${REQUIRED_NODE_MAJOR}\` (or an equivalent version manager command) and reinstall dependencies.`);
}

function loadBetterSqlite3() {
  const modulePath = require.resolve("better-sqlite3");
  delete require.cache[modulePath];
  return require("better-sqlite3");
}

function rebuildNativeDependency() {
  const env = {
    ...process.env,
    KANVIBE_NATIVE_REBUILD_ATTEMPTED: "1",
  };

  if (isElectronRuntime) {
    if (isPackagedElectronRuntime()) {
      throw new Error("better-sqlite3 ABI mismatch in packaged Electron runtime");
    }

    console.warn("[kanvibe] Detected Electron native ABI mismatch. Rebuilding app dependencies for Electron...");
    execFileSync("pnpm", ["exec", "electron-builder", "install-app-deps"], {
      stdio: "inherit",
      env,
    });
    return;
  }

  console.warn("[kanvibe] Detected Node native ABI mismatch. Rebuilding better-sqlite3...");
  execFileSync("pnpm", ["rebuild", "better-sqlite3"], {
    stdio: "inherit",
    env,
  });
}

function printRebuildFailureGuidance(error) {
  console.error("[kanvibe] better-sqlite3 is still not loadable after an automatic rebuild attempt.");
  console.error(String(error?.stack || error?.message || error));
  if (isElectronRuntime) {
    if (isPackagedElectronRuntime()) {
      console.error("[kanvibe] This packaged app cannot rebuild native modules on the end-user machine.");
      console.error("[kanvibe] Reinstall or re-download a release built for this Electron version and platform.");
      return;
    }

    console.error("[kanvibe] Try running `pnpm exec electron-builder install-app-deps` and launch the desktop app again.");
    return;
  }

  console.error("[kanvibe] Try running `pnpm rebuild better-sqlite3` (or `rm -rf node_modules && pnpm install`) under Node 24.x.");
}

function main() {
  if (!isElectronRuntime && getNodeMajor() !== REQUIRED_NODE_MAJOR) {
    printNodeVersionGuidance();
    process.exit(1);
  }

  try {
    loadBetterSqlite3();
  } catch (error) {
    if (!isNativeModuleMismatch(error)) {
      throw error;
    }

    if (hasAttemptedRebuild) {
      printRebuildFailureGuidance(error);
      process.exit(1);
    }

    try {
      rebuildNativeDependency();
    } catch (rebuildError) {
      printRebuildFailureGuidance(rebuildError);
      process.exit(1);
    }

    try {
      loadBetterSqlite3();
    } catch (retryError) {
      printRebuildFailureGuidance(retryError);
      process.exit(1);
    }
  }
}

main();
