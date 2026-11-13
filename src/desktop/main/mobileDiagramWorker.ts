import { access } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { MOBILE_DIAGRAM_LIMITS, MobileDiagramError, type MobileDiagramInput } from "./mobileDiagramContract";

export function isDiagramWorkerResource(url: string, rendererRoot: string): boolean {
  try {
    const parsed = new URL(url);
    if (parsed.protocol !== "file:" || parsed.host || parsed.search || parsed.hash) return false;
    const file = fileURLToPath(parsed);
    const relative = path.relative(rendererRoot, file);
    return relative === "diagram-worker.html" || (relative.startsWith(`assets${path.sep}`) && !relative.split(path.sep).includes(".."));
  } catch {
    return false;
  }
}

export async function renderDiagramInDesktop(input: MobileDiagramInput): Promise<Buffer> {
  const { app, BrowserWindow, session, nativeImage } = await import("electron");
  if (!app.isReady()) throw new MobileDiagramError(503, "Desktop 다이어그램 렌더러를 사용할 수 없습니다");
  const rendererRoot = path.join(app.getAppPath(), "build", "renderer");
  const documentPath = path.join(rendererRoot, "diagram-worker.html");
  try {
    await access(documentPath);
  } catch {
    throw new MobileDiagramError(503, "Desktop 다이어그램 렌더러를 사용할 수 없습니다");
  }
  const isolated = session.fromPartition("kanvibe-diagram-renderer");
  isolated.setPermissionRequestHandler((_contents, _permission, callback) => callback(false));
  isolated.setPermissionCheckHandler(() => false);
  isolated.webRequest.onBeforeRequest((details, callback) => callback({ cancel: !isDiagramWorkerResource(details.url, rendererRoot) }));

  const { registerDiagramWorkerWindow } = await import(path.join(app.getAppPath(), "electron", "diagramWorkerWindows.js"));
  let window: InstanceType<typeof BrowserWindow>;
  try {
    window = new BrowserWindow({
    show: false, focusable: false, skipTaskbar: true, useContentSize: true,
    width: input.width, height: 2400,
    webPreferences: {
      session: isolated,
      preload: path.join(app.getAppPath(), "electron", "diagramWorkerPreload.js"),
      sandbox: true, contextIsolation: true, nodeIntegration: false, webSecurity: true,
      allowRunningInsecureContent: false, backgroundThrottling: false, offscreen: true,
      spellcheck: false, devTools: false,
    },
  });
  } catch {
    throw new MobileDiagramError(503, "Desktop 다이어그램 렌더러를 사용할 수 없습니다");
  }
  registerDiagramWorkerWindow(window);
  const contents = window.webContents;
  const ipc = contents.ipc;
  contents.setWindowOpenHandler(() => ({ action: "deny" }));
  contents.on("will-navigate", (event) => event.preventDefault());
  contents.on("will-frame-navigate", (event) => event.preventDefault());
  contents.on("will-attach-webview", (event) => event.preventDefault());

  return new Promise<Buffer>((resolve, reject) => {
    let settled = false;
    let capturing = false;
    const cleanup = () => {
      clearTimeout(timeout);
      app.removeListener("before-quit", quit);
      ipc.removeAllListeners("kanvibe:diagram:ready");
      ipc.removeAllListeners("kanvibe:diagram:complete");
      if (!contents.isDestroyed()) contents.removeAllListeners("paint");
      // Renderer 종료와 native 객체 해제가 겹쳐도 대기열의 Promise는 반드시 끝낸다.
      try { if (!window.isDestroyed()) window.destroy(); } catch { /* 이미 닫힌 소유 창 */ }
    };
    const finish = (error?: MobileDiagramError, png?: Buffer) => {
      if (settled) return;
      settled = true;
      cleanup();
      if (error) reject(error); else resolve(png!);
    };
    const unavailable = () => finish(new MobileDiagramError(503, "Desktop 다이어그램 렌더러를 사용할 수 없습니다"));
    const quit = () => unavailable();
    const timeout = setTimeout(() => finish(new MobileDiagramError(503, "다이어그램 렌더 시간이 초과되었습니다")), MOBILE_DIAGRAM_LIMITS.timeoutMs);
    app.once("before-quit", quit);
    contents.once("render-process-gone", unavailable);
    contents.once("destroyed", () => { if (!settled) unavailable(); });
    window.once("closed", () => { if (!settled) unavailable(); });
    contents.ipc.once("kanvibe:diagram:ready", (event) => {
      if (event.senderFrame !== contents.mainFrame) return unavailable();
      contents.send("kanvibe:diagram:render", input);
    });
    contents.ipc.on("kanvibe:diagram:complete", async (event, result: unknown) => {
      if (settled || capturing) return;
      if (event.senderFrame !== contents.mainFrame || !result || typeof result !== "object") return unavailable();
      const { success, height } = result as { success?: unknown; height?: unknown };
      if (success !== true) return finish(new MobileDiagramError(422, "다이어그램 문법을 확인해 주세요"));
      if (typeof height !== "number" || !Number.isInteger(height) || height < 1 || height > 2400) return unavailable();
      capturing = true;
      try {
        // DOM 완료 전에 대기하던 흰 compositor 프레임은 건너뛰고 실제 그래프가 그려진 프레임을 캡처한다.
        let image: Electron.NativeImage;
        while (true) {
          const frame = await new Promise<Electron.NativeImage>((ready) => {
            contents.once("paint", (_paintEvent, _dirty, image) => ready(image));
            contents.invalidate();
          });
          if (settled) return;
          image = frame.crop({ x: 0, y: 0, width: input.width, height });
          const bitmap = image.toBitmap({ scaleFactor: 1 });
          const first = bitmap.subarray(0, 4);
          let painted = false;
          for (let offset = 4; offset < bitmap.length; offset += 4) {
            if (!bitmap.subarray(offset, offset + 4).equals(first)) { painted = true; break; }
          }
          if (painted) break;
          await new Promise<void>((ready) => setImmediate(ready));
        }
        // 불투명 문서의 JPEG를 사용해 실제 Android PNG decoder 호환 문제를 피한다.
        const encoded = image.toJPEG(100);
        const decoded = nativeImage.createFromBuffer(encoded);
        const size = decoded.getSize();
        if (decoded.isEmpty() || size.width !== input.width || size.height !== height) return unavailable();
        finish(undefined, encoded);
      } catch {
        unavailable();
      }
    });
    void contents.loadFile(documentPath).catch(unavailable);
  });
}
