/* eslint-disable @typescript-eslint/no-require-imports */

const fs = require("node:fs");
const path = require("node:path");
const process = require("node:process");

const DEFAULT_DESKTOP_LOG_MAX_BYTES = 10 * 1024 * 1024;
const DEFAULT_DESKTOP_LOG_TRIM_TO_BYTES = 8 * 1024 * 1024;
const DEFAULT_DESKTOP_LOG_MAX_ENTRY_BYTES = 256 * 1024;

function resolveDesktopLogPath(userDataPath) {
  return path.join(userDataPath, "logs", "kanvibe-desktop.log");
}

function toPositiveInteger(value, fallback) {
  if (Number.isFinite(value) && value > 0) {
    return Math.floor(value);
  }

  return fallback;
}

function getFileSize(filePath) {
  try {
    return fs.statSync(filePath).size;
  } catch (error) {
    if (error && error.code === "ENOENT") {
      return 0;
    }

    throw error;
  }
}

function trimDesktopLogHead(logPath, bytesToRemove) {
  if (bytesToRemove <= 0 || !fs.existsSync(logPath)) {
    return;
  }

  const content = fs.readFileSync(logPath);
  if (content.length <= bytesToRemove) {
    fs.writeFileSync(logPath, "", "utf8");
    return;
  }

  fs.writeFileSync(logPath, content.subarray(bytesToRemove));
}

function trimDesktopLogIfNeeded(logPath, incomingBytes, maxFileBytes, trimToBytes) {
  const currentBytes = getFileSize(logPath);
  if (currentBytes + incomingBytes <= maxFileBytes) {
    return;
  }

  const bytesToKeepBeforeAppend = Math.max(0, trimToBytes - incomingBytes);
  trimDesktopLogHead(logPath, currentBytes - bytesToKeepBeforeAppend);
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

function getByteLength(value) {
  return Buffer.byteLength(value, "utf8");
}

function buildLogLine(timestamp, event, entry) {
  return `[${timestamp}] ${event} ${JSON.stringify(entry)}\n`;
}

function constrainLogLineBytes(timestamp, event, entry, maxEntryBytes) {
  const line = buildLogLine(timestamp, event, entry);
  const originalBytes = getByteLength(line);
  if (originalBytes <= maxEntryBytes) {
    return {
      line,
      lineBytes: originalBytes,
    };
  }

  const truncatedLine = buildLogLine(timestamp, event, {
    diagnosticPayloadTruncated: true,
    originalBytes,
    maxEntryBytes,
    process: entry.process,
  });

  return {
    line: truncatedLine,
    lineBytes: getByteLength(truncatedLine),
  };
}

function createDesktopDiagnostics(options) {
  const logPath = options.logPath;
  const getTimestamp = options.getTimestamp || (() => new Date().toISOString());
  const getProcessMeta = options.getProcessMeta || defaultProcessMeta;
  const maxFileBytes = toPositiveInteger(options.maxFileBytes, DEFAULT_DESKTOP_LOG_MAX_BYTES);
  const trimToBytes = Math.min(
    toPositiveInteger(options.trimToBytes, DEFAULT_DESKTOP_LOG_TRIM_TO_BYTES),
    maxFileBytes,
  );
  const maxEntryBytes = Math.min(
    toPositiveInteger(options.maxEntryBytes, DEFAULT_DESKTOP_LOG_MAX_ENTRY_BYTES),
    maxFileBytes,
  );

  function log(event, payload = {}) {
    try {
      fs.mkdirSync(path.dirname(logPath), { recursive: true });
      const entry = {
        ...normalizePayload(payload),
        process: getProcessMeta(),
      };
      const { line, lineBytes } = constrainLogLineBytes(getTimestamp(), event, entry, maxEntryBytes);
      trimDesktopLogIfNeeded(logPath, lineBytes, maxFileBytes, trimToBytes);
      fs.appendFileSync(logPath, line, "utf8");
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
  DEFAULT_DESKTOP_LOG_MAX_BYTES,
  DEFAULT_DESKTOP_LOG_MAX_ENTRY_BYTES,
  DEFAULT_DESKTOP_LOG_TRIM_TO_BYTES,
  createDesktopDiagnostics,
  resolveDesktopLogPath,
  serializeErrorForLog,
};
