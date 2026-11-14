import type { EventEmitter } from "node:events";
import { copyFileSync, mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { afterAll, afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type ContentsProbe = EventEmitter & { ipc: EventEmitter; mainFrame: object; invalidate: ReturnType<typeof vi.fn> };
type WindowProbe = { closed: boolean; contentClosed: boolean; options: Electron.BrowserWindowConstructorOptions; webContents: ContentsProbe };
type AppProbe = EventEmitter & { isReady: () => boolean; getAppPath: () => string };
const fake = vi.hoisted(() => ({ root: "", failView: false, app: undefined as unknown as AppProbe, windows: [] as WindowProbe[], views: [] as WindowProbe[], session: { setPermissionRequestHandler: vi.fn(), setPermissionCheckHandler: vi.fn(), webRequest: { onBeforeRequest: vi.fn() } } }));
vi.mock("electron", async () => {
  const { EventEmitter } = await import("node:events");
  fake.app = Object.assign(new EventEmitter(), { isReady: () => true, getAppPath: () => "" });
  class BrowserWindow extends EventEmitter {
    closed = false;
    contentClosed = false;
    webContents = Object.assign(new EventEmitter(), { ipc: new EventEmitter(), mainFrame: {}, setWindowOpenHandler: vi.fn(), send: vi.fn(), loadFile: vi.fn(async () => undefined), invalidate: vi.fn(), isDestroyed: () => this.contentClosed });
    constructor(public options: Electron.BrowserWindowConstructorOptions) {
      super();
      if (fake.failView) throw new Error("native constructor failed");
      fake.windows.push(this); fake.views.push(this);
    }
    isDestroyed() { return this.closed; }
    destroy() { this.closed = true; this.contentClosed = true; this.webContents.emit("destroyed"); this.emit("closed"); }
  }
  return { BrowserWindow, app: fake.app, session: { fromPartition: () => fake.session }, nativeImage: { createFromBuffer: () => ({ isEmpty: () => false, getSize: () => ({ width: 640, height: 100 }) }) } };
});
import { isDiagramWorkerResource, renderDiagramInDesktop } from "../mobileDiagramWorker";

const root = mkdtempSync(path.join(os.tmpdir(), "kanvibe-diagram-test-"));
mkdirSync(path.join(root, "build/renderer"), { recursive: true });
writeFileSync(path.join(root, "build/renderer/diagram-worker.html"), "<html></html>");
mkdirSync(path.join(root, "electron"));
copyFileSync(path.join(process.cwd(), "electron/diagramWorkerWindows.js"), path.join(root, "electron/diagramWorkerWindows.js"));
beforeEach(async () => { await import("electron"); vi.clearAllMocks(); fake.failView = false; fake.windows.length = 0; fake.views.length = 0; fake.app.getAppPath = () => root; });
afterEach(() => { vi.useRealTimers(); });
afterAll(() => rmSync(root, { force: true, recursive: true }));
const input = { definition: "flowchart LR\nA-->B", theme: "dark" as const, width: 640 };

describe("isolated diagram worker", () => {
  it("skips an initial blank frame, captures painted pixels and destroys the worker before resolving", async () => {
    const pending = renderDiagramInDesktop(input);
    await vi.waitFor(() => expect(fake.views).toHaveLength(1));
    const contents = fake.views[0].webContents;
    const png = Buffer.from("native image capture");
    const blank = { crop: () => ({ toBitmap: () => Buffer.alloc(16), toJPEG: () => png }) };
    const painted = { crop: () => ({ toBitmap: () => Buffer.from([255,255,255,255,0,0,0,255]), toJPEG: () => png }) };
    let frames = 0;
    contents.invalidate.mockImplementation(() => { frames++; contents.emit("paint", {}, {}, frames === 1 ? blank : painted); });
    contents.ipc.emit("kanvibe:diagram:complete", { senderFrame: contents.mainFrame }, { success: true, height: 100 });
    expect(await pending).toEqual(png);
    expect(frames).toBe(2);
    expect(fake.windows[0].closed).toBe(true);
    expect(fake.app.listenerCount("before-quit")).toBe(0);
  });

  it("does not retain a worker when native window construction fails", async () => {
    fake.failView = true;
    await expect(renderDiagramInDesktop(input)).rejects.toMatchObject({ statusCode: 503 });
    expect(fake.windows).toHaveLength(0);
  });
  it("finishes safely after the renderer native object is already destroyed", async () => {
    const pending = renderDiagramInDesktop(input);
    const result = expect(pending).rejects.toMatchObject({ statusCode: 503 });
    await vi.waitFor(() => expect(fake.views).toHaveLength(1));
    const view = fake.views[0];
    view.contentClosed = true;
    Object.defineProperty(view.webContents, "ipc", { get() { throw new Error("Object has been destroyed"); } });
    view.webContents.emit("destroyed");
    await result;
    expect(fake.windows[0].closed).toBe(true);
  });
  it("aborts its active render during application shutdown", async () => {
    const pending = renderDiagramInDesktop(input);
    const result = expect(pending).rejects.toMatchObject({ statusCode: 503 });
    await vi.waitFor(() => expect(fake.views).toHaveLength(1));
    fake.app.emit("before-quit");
    await result;
    expect(fake.windows[0].closed).toBe(true);
    expect(fake.app.listenerCount("before-quit")).toBe(0);
  });

  it("permits only its packaged document/assets and rejects other local/network resources", () => {
    const renderer = path.join(root, "build/renderer");
    expect(isDiagramWorkerResource(pathToFileURL(path.join(renderer, "diagram-worker.html")).href, renderer)).toBe(true);
    expect(isDiagramWorkerResource(pathToFileURL(path.join(renderer, "assets/chunk.js")).href, renderer)).toBe(true);
    expect(isDiagramWorkerResource(pathToFileURL(path.join(root, "private.txt")).href, renderer)).toBe(false);
    expect(isDiagramWorkerResource(pathToFileURL(path.join(renderer, "index.html")).href, renderer)).toBe(false);
    expect(isDiagramWorkerResource("https://example.com/assets/chunk.js", renderer)).toBe(false);
    expect(isDiagramWorkerResource("file://server/assets/chunk.js", renderer)).toBe(false);
  });
  it("destroys only its worker on the 15-second deadline and rejects the request", async () => {
    vi.useFakeTimers();
    const pending = renderDiagramInDesktop(input);
    const result = expect(pending).rejects.toMatchObject({ statusCode: 503 });
    await vi.waitFor(() => expect(fake.views).toHaveLength(1));
    expect(fake.views[0].options.webPreferences).toMatchObject({ sandbox: true, contextIsolation: true, nodeIntegration: false, devTools: false });
    await vi.advanceTimersByTimeAsync(15000);
    await result;
    expect(fake.windows[0].closed).toBe(true);
    expect(fake.views[0].closed).toBe(true);
    expect(fake.app.listenerCount("before-quit")).toBe(0);
  });
  it("closes the owning hidden window when its renderer crashes", async () => {
    const pending = renderDiagramInDesktop(input);
    const result = expect(pending).rejects.toMatchObject({ statusCode: 503 });
    await vi.waitFor(() => expect(fake.views).toHaveLength(1));
    fake.views[0].webContents.emit("render-process-gone", {}, { reason: "crashed" });
    await result;
    expect(fake.windows[0].closed).toBe(true);
    expect(fake.views[0].closed).toBe(true);
  });
  it("accepts completion only from its main frame", async () => {
    const pending = renderDiagramInDesktop(input);
    const result = expect(pending).rejects.toMatchObject({ statusCode: 503 });
    await vi.waitFor(() => expect(fake.views).toHaveLength(1));
    fake.views[0].webContents.ipc.emit("kanvibe:diagram:complete", { senderFrame: {} }, { success: true, height: 100 });
    await result;
    expect(fake.views[0].webContents.invalidate).not.toHaveBeenCalled();
  });
  it("maps syntax failures to 422 without capturing or retaining a window", async () => {
    const pending = renderDiagramInDesktop(input);
    const result = expect(pending).rejects.toMatchObject({ statusCode: 422 });
    await vi.waitFor(() => expect(fake.views).toHaveLength(1));
    const contents = fake.views[0].webContents;
    contents.ipc.emit("kanvibe:diagram:complete", { senderFrame: contents.mainFrame }, { success: false });
    await result;
    expect(contents.invalidate).not.toHaveBeenCalled();
    expect(fake.windows[0].closed).toBe(true);
  });
});
