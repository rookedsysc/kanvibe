#!/usr/bin/env node

const { execFileSync } = require("node:child_process");

const REQUIRED_NODE_MAJOR = 24;

function getNodeMajor() {
  return Number.parseInt(process.versions.node.split(".")[0] || "0", 10);
}

function hasBetterSqlite3Installed() {
  try {
    require.resolve("better-sqlite3");
    return true;
  } catch {
    return false;
  }
}

function ensureSupportedNodeVersion() {
  if (getNodeMajor() === REQUIRED_NODE_MAJOR) {
    return;
  }

  console.error(
    `[kanvibe] Unsupported Node.js runtime ${process.versions.node}. KanVibe desktop dev requires Node ${REQUIRED_NODE_MAJOR}.x.`,
  );
  console.error(`[kanvibe] Run \`nvm use ${REQUIRED_NODE_MAJOR}\` and then retry \`pnpm desktop:dev\`.`);
  process.exit(1);
}

function installElectronNativeDependencies() {
  console.warn("[kanvibe] Rebuilding native dependencies for the Electron runtime...");
  execFileSync("pnpm", ["exec", "electron-builder", "install-app-deps"], {
    stdio: "inherit",
    env: process.env,
  });
}

function installProjectDependencies() {
  console.warn("[kanvibe] Installing project dependencies because better-sqlite3 is missing...");
  execFileSync("pnpm", ["install"], {
    stdio: "inherit",
    env: process.env,
  });
}

function launchElectron() {
  execFileSync("pnpm", ["exec", "electron", "."], {
    stdio: "inherit",
    env: process.env,
  });
}

function main() {
  ensureSupportedNodeVersion();

  if (!hasBetterSqlite3Installed()) {
    installProjectDependencies();
  }

  installElectronNativeDependencies();
  launchElectron();
}

main();
