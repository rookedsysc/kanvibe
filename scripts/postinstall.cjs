#!/usr/bin/env node

const { chmodSync, existsSync } = require("node:fs");
const { execFileSync } = require("node:child_process");
const path = require("node:path");

const REQUIRED_NODE_MAJOR = 24;

function getNodeMajor() {
  return Number.parseInt(process.versions.node.split(".")[0] || "0", 10);
}

function chmodNodePtySpawnHelper() {
  const helperPath = path.join(
    process.cwd(),
    "node_modules",
    "node-pty",
    "prebuilds",
    "darwin-arm64",
    "spawn-helper",
  );

  if (!existsSync(helperPath)) {
    return;
  }

  try {
    chmodSync(helperPath, 0o755);
  } catch {
    // 권한 보정 실패는 개발 환경에 따라 발생할 수 있어 설치를 막지 않는다.
  }
}

function hasElectronBuilderInstalled() {
  try {
    require.resolve("electron-builder");
    return true;
  } catch {
    return false;
  }
}

function getPackageExecCommand() {
  const npmExecPath = process.env.npm_execpath || "";

  if (npmExecPath.includes("pnpm")) {
    return { command: "pnpm", args: ["exec"] };
  }

  if (npmExecPath.includes("yarn")) {
    return { command: "yarn", args: ["exec"] };
  }

  if (npmExecPath.includes("bun")) {
    return { command: "bunx", args: [] };
  }

  return { command: process.platform === "win32" ? "npx.cmd" : "npx", args: ["--no-install"] };
}

function rebuildElectronNativeDependencies() {
  const packageExec = getPackageExecCommand();

  console.warn("[kanvibe] Postinstall: rebuilding native dependencies for the Electron runtime...");
  execFileSync(packageExec.command, [...packageExec.args, "electron-rebuild", "-f", "--only", "better-sqlite3"], {
    stdio: "inherit",
    env: process.env,
  });
}

function main() {
  chmodNodePtySpawnHelper();

  if (getNodeMajor() !== REQUIRED_NODE_MAJOR) {
    console.warn(
      `[kanvibe] Postinstall: skipping Electron native rebuild because Node ${process.versions.node} is active. Use Node ${REQUIRED_NODE_MAJOR}.x and run \`pnpm exec electron-rebuild -f --only better-sqlite3\` if needed.`,
    );
    return;
  }

  if (!hasElectronBuilderInstalled()) {
    return;
  }

  try {
    rebuildElectronNativeDependencies();
  } catch (error) {
    console.warn("[kanvibe] Postinstall: Electron native rebuild failed.");
    console.warn(String(error?.message || error));
  }
}

main();
