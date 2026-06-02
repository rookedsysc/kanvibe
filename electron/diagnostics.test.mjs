import fs from "node:fs";
import { createRequire } from "node:module";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);
const tempDirs = [];

function createTempLogPath() {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "kanvibe-diagnostics-"));
  tempDirs.push(tempDir);
  return path.join(tempDir, "logs", "kanvibe-desktop.log");
}

function createDiagnostics(logPath, options = {}) {
  const { createDesktopDiagnostics } = require("./diagnostics");
  return createDesktopDiagnostics({
    logPath,
    getTimestamp: () => "2026-01-01T00:00:00.000Z",
    getProcessMeta: () => ({ pid: 42, platform: "test" }),
    ...options,
  });
}

function getFileSize(filePath) {
  return fs.statSync(filePath).size;
}

afterEach(() => {
  while (tempDirs.length > 0) {
    const tempDir = tempDirs.pop();
    fs.rmSync(tempDir, { recursive: true, force: true });
  }
});

describe("Electron desktop diagnostics", () => {
  it("keeps one append-only diagnostics file by trimming bytes from the head", () => {
    const logPath = createTempLogPath();
    const maxFileBytes = 260;
    const diagnostics = createDiagnostics(logPath, {
      maxFileBytes,
      trimToBytes: 180,
      maxEntryBytes: 220,
    });

    for (let index = 0; index < 8; index += 1) {
      diagnostics.log("renderer:console-message", {
        index,
        message: `entry-${index}-${"x".repeat(20)}`,
      });
    }

    const content = fs.readFileSync(logPath, "utf8");
    expect(content).not.toContain("entry-0");
    expect(content).toContain("entry-7");
    expect(getFileSize(logPath)).toBeLessThanOrEqual(maxFileBytes);
    expect(fs.existsSync(`${logPath}.1`)).toBe(false);
  });

  it("trims an oversized legacy log instead of rotating it into a backup", () => {
    const logPath = createTempLogPath();
    const maxFileBytes = 220;
    fs.mkdirSync(path.dirname(logPath), { recursive: true });
    fs.writeFileSync(logPath, `legacy-start\n${"legacy-middle".repeat(20)}\nlegacy-tail\n`, "utf8");
    const diagnostics = createDiagnostics(logPath, {
      maxFileBytes,
      trimToBytes: 150,
      maxEntryBytes: 180,
    });

    diagnostics.log("main:start", { ok: true });

    const content = fs.readFileSync(logPath, "utf8");
    expect(content).not.toContain("legacy-start");
    expect(content).toContain("main:start");
    expect(getFileSize(logPath)).toBeLessThanOrEqual(maxFileBytes);
    expect(fs.existsSync(`${logPath}.1`)).toBe(false);
  });

  it("truncates oversized diagnostic entries before appending them", () => {
    const logPath = createTempLogPath();
    const diagnostics = createDiagnostics(logPath, {
      maxFileBytes: 1024,
      trimToBytes: 768,
      maxEntryBytes: 512,
    });

    diagnostics.log("renderer:console-message", {
      message: "x".repeat(5000),
    });

    const content = fs.readFileSync(logPath, "utf8");
    expect(content).toContain("diagnosticPayloadTruncated");
    expect(content).toContain("originalBytes");
    expect(content).not.toContain("x".repeat(1000));
    expect(getFileSize(logPath)).toBeLessThan(1024);
  });
});
