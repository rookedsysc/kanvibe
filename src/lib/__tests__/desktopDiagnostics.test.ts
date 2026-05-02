import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { afterEach, describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);
const tempDirs: string[] = [];

interface DesktopDiagnosticsModule {
  createDesktopDiagnostics: (options: {
    logPath: string;
    getTimestamp?: () => string;
    getProcessMeta?: () => Record<string, unknown>;
  }) => {
    logPath: string;
    log: (event: string, payload?: Record<string, unknown>) => void;
  };
  resolveDesktopLogPath: (userDataPath: string) => string;
  serializeErrorForLog: (error: unknown) => string;
}

function readSource(relativePath: string): string {
  return readFileSync(path.join(process.cwd(), relativePath), "utf8");
}

function loadDiagnosticsModule(): DesktopDiagnosticsModule {
  return require(path.join(process.cwd(), "electron", "diagnostics.js")) as DesktopDiagnosticsModule;
}

function createTempDir(): string {
  const tempDir = mkdtempSync(path.join(tmpdir(), "kanvibe-diagnostics-test-"));
  tempDirs.push(tempDir);
  return tempDir;
}

afterEach(() => {
  for (const tempDir of tempDirs.splice(0)) {
    rmSync(tempDir, { recursive: true, force: true });
  }
});

describe("desktop diagnostics", () => {
  it("writes structured diagnostic events to the packaged app log path", () => {
    const diagnosticsModule = loadDiagnosticsModule();
    const userDataPath = createTempDir();
    const logPath = diagnosticsModule.resolveDesktopLogPath(userDataPath);
    const diagnostics = diagnosticsModule.createDesktopDiagnostics({
      logPath,
      getTimestamp: () => "2026-05-02T07:30:00.000Z",
      getProcessMeta: () => ({ pid: 1234, platform: "darwin", arch: "arm64" }),
    });

    diagnostics.log("renderer:load-start", { url: "file:///KanVibe/index.html#/ko/login" });

    const log = readFileSync(logPath, "utf8");
    expect(diagnostics.logPath).toBe(path.join(userDataPath, "logs", "kanvibe-desktop.log"));
    expect(log).toContain("[2026-05-02T07:30:00.000Z] renderer:load-start");
    expect(log).toContain('"url":"file:///KanVibe/index.html#/ko/login"');
    expect(log).toContain('"pid":1234');
    expect(log).toContain('"platform":"darwin"');
  });

  it("serializes thrown values into readable log payloads", () => {
    const diagnosticsModule = loadDiagnosticsModule();

    expect(diagnosticsModule.serializeErrorForLog(new Error("native module failed"))).toContain("native module failed");
    expect(diagnosticsModule.serializeErrorForLog("string failure")).toBe("string failure");
  });

  it("wires main process diagnostics around startup, renderer loading, and IPC failures", () => {
    const source = readSource("electron/main.js");

    expect(source).toContain("createDesktopDiagnostics");
    expect(source).toContain('diagnostics.log("main:startup"');
    expect(source).toContain('process.on("uncaughtException"');
    expect(source).toContain('process.on("unhandledRejection"');
    expect(source).toContain('ipcMain.on("kanvibe:renderer-log"');
    expect(source).toContain('logDiagnostic("ipc:invoke-start"');
    expect(source).toContain('logDiagnostic("ipc:invoke-succeeded"');
    expect(source).toContain('logDiagnostic("ipc:invoke-failed"');
    expect(source).toContain('webContents.on("did-fail-load"');
    expect(source).toContain('webContents.on("render-process-gone"');
    expect(source).toContain('webContents.on("console-message"');
    expect(source).toContain('webContents.on("preload-error"');
  });

  it("wires preload and renderer diagnostics before React bootstraps", () => {
    const preloadSource = readSource("electron/preload.js");
    const rendererSource = readSource("src/desktop/renderer/main.tsx");
    const globalTypes = readSource("src/desktop/renderer/global.d.ts");

    expect(preloadSource).toContain('ipcRenderer.send("kanvibe:renderer-log"');
    expect(preloadSource).toContain("logRendererError");
    expect(preloadSource).toContain('window.addEventListener("error"');
    expect(preloadSource).toContain('window.addEventListener("unhandledrejection"');
    expect(rendererSource).toContain("installRendererDiagnostics");
    expect(rendererSource).toContain("window.kanvibeDesktop?.logRendererError");
    expect(rendererSource.indexOf("installRendererDiagnostics();")).toBeLessThan(rendererSource.indexOf("createRoot(container)"));
    expect(globalTypes).toContain("logRendererError?:");
  });
});
