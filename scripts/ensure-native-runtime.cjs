#!/usr/bin/env node

const { execFileSync } = require("node:child_process");
const path = require("node:path");

const REQUIRED_NODE_MAJOR = 24;
const isRunningInsideElectron = Boolean(process.versions.electron);
const isElectronTarget = isRunningInsideElectron || process.argv.includes("--electron");
const hasAttemptedRebuild = process.env.KANVIBE_NATIVE_REBUILD_ATTEMPTED === "1";

function isPackagedElectronRuntime() {
  return isRunningInsideElectron && !process.defaultApp;
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

function verifyNodeBetterSqlite3Binding() {
  try {
    execFileSync(process.execPath, [path.join(__dirname, "verify-node-better-sqlite3.cjs")], {
      stdio: ["ignore", "inherit", "pipe"],
      env: process.env,
    });
  } catch (error) {
    const stderr = error?.stderr?.toString?.() || "";
    throw new Error(stderr || String(error?.message || error));
  }
}

function verifyElectronBetterSqlite3Binding() {
  execFileSync("pnpm", ["exec", "electron", path.join(__dirname, "verify-electron-better-sqlite3.cjs")], {
    stdio: "inherit",
    env: process.env,
  });
}

function verifyBetterSqlite3Binding() {
  if (isElectronTarget && !isRunningInsideElectron) {
    verifyElectronBetterSqlite3Binding();
    return;
  }

  verifyNodeBetterSqlite3Binding();
}

function verifyFreshNodeRuntimeAfterRebuild() {
  const previousAttempted = process.env.KANVIBE_NATIVE_REBUILD_ATTEMPTED;
  process.env.KANVIBE_NATIVE_REBUILD_ATTEMPTED = "1";
  verifyNodeBetterSqlite3Binding();
  if (previousAttempted === undefined) {
    delete process.env.KANVIBE_NATIVE_REBUILD_ATTEMPTED;
  } else {
    process.env.KANVIBE_NATIVE_REBUILD_ATTEMPTED = previousAttempted;
  }
  process.exit(0);
}

function verifyBetterSqlite3BindingAfterRebuild() {
  if (!isElectronTarget && !isRunningInsideElectron) {
    verifyFreshNodeRuntimeAfterRebuild();
    return;
  }

  verifyBetterSqlite3Binding();
}

function rebuildNativeDependency() {
  const env = {
    ...process.env,
    KANVIBE_NATIVE_REBUILD_ATTEMPTED: "1",
  };

  if (isElectronTarget) {
    if (isPackagedElectronRuntime()) {
      throw new Error("better-sqlite3 ABI mismatch in packaged Electron runtime");
    }

    console.warn("[kanvibe] Detected Electron native ABI mismatch. Rebuilding better-sqlite3 for Electron from source...");
    execFileSync("pnpm", ["exec", "electron-rebuild", "-f", "--build-from-source", "--only", "better-sqlite3"], {
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
  if (isElectronTarget) {
    if (isPackagedElectronRuntime()) {
      console.error("[kanvibe] This packaged app cannot rebuild native modules on the end-user machine.");
      console.error("[kanvibe] Reinstall or re-download a release built for this Electron version and platform.");
      return;
    }

    console.error("[kanvibe] Try running `pnpm exec electron-rebuild -f --build-from-source --only better-sqlite3` and launch the desktop app again.");
    return;
  }

  console.error("[kanvibe] Try running `pnpm rebuild better-sqlite3` (or `rm -rf node_modules && pnpm install`) under Node 24.x.");
}

function main() {
  if (!isRunningInsideElectron && getNodeMajor() !== REQUIRED_NODE_MAJOR) {
    printNodeVersionGuidance();
    process.exit(1);
  }

  try {
    verifyBetterSqlite3Binding();
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
      verifyBetterSqlite3BindingAfterRebuild();
    } catch (retryError) {
      printRebuildFailureGuidance(retryError);
      process.exit(1);
    }
  }
}

main();
