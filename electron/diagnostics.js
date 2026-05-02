/* eslint-disable @typescript-eslint/no-require-imports */

const fs = require("node:fs");
const path = require("node:path");
const process = require("node:process");

function resolveDesktopLogPath(userDataPath) {
  return path.join(userDataPath, "logs", "kanvibe-desktop.log");
}

function serializeErrorForLog(error) {
  if (error instanceof Error) {
    return error.stack || `${error.name}: ${error.message}`;
  }

  if (typeof error === "string") {
    return error;
  }

  try {
    return JSON.stringify(error);
  } catch {
    return String(error);
  }
}

function normalizePayload(value, seen = new WeakSet()) {
  if (value instanceof Error) {
    return serializeErrorForLog(value);
  }

  if (value === null || typeof value !== "object") {
    return value;
  }

  if (seen.has(value)) {
    return "[Circular]";
  }

  seen.add(value);

  if (Array.isArray(value)) {
    return value.map((entry) => normalizePayload(entry, seen));
  }

  return Object.fromEntries(
    Object.entries(value).map(([key, entry]) => [key, normalizePayload(entry, seen)]),
  );
}

function defaultProcessMeta() {
  return {
    pid: process.pid,
    platform: process.platform,
    arch: process.arch,
    node: process.versions.node,
    electron: process.versions.electron || null,
  };
}

function createDesktopDiagnostics(options) {
  const logPath = options.logPath;
  const getTimestamp = options.getTimestamp || (() => new Date().toISOString());
  const getProcessMeta = options.getProcessMeta || defaultProcessMeta;

  function log(event, payload = {}) {
    try {
      fs.mkdirSync(path.dirname(logPath), { recursive: true });
      const entry = {
        ...normalizePayload(payload),
        process: getProcessMeta(),
      };
      fs.appendFileSync(logPath, `[${getTimestamp()}] ${event} ${JSON.stringify(entry)}\n`, "utf8");
    } catch (error) {
      console.error("[kanvibe] failed to write desktop diagnostics log:", error);
    }
  }

  return {
    logPath,
    log,
  };
}

module.exports = {
  createDesktopDiagnostics,
  resolveDesktopLogPath,
  serializeErrorForLog,
};
