import { afterEach, describe, expect, it, vi } from "vitest";

const { invoke, listen, isTauri, callbacks, pending } = vi.hoisted(() => ({
  invoke: vi.fn().mockResolvedValue(null),
  listen: vi.fn(),
  isTauri: vi.fn().mockReturnValue(true),
  callbacks: new Map<string, (event: { payload: unknown }) => void>(),
  pending: [] as (() => void)[],
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke, isTauri }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));

import { initializeDesktopBridge } from "../desktopBridge";

afterEach(() => {
  window.dispatchEvent(new Event("pagehide"));
  vi.clearAllMocks();
  callbacks.clear();
  pending.length = 0;
});

describe("Tauri desktop bridge", () => {
  it("새 창 단축키는 빈 보드와 같은 경로에서도 매번 강제 생성 명령을 보낸다", async () => {
    listen.mockResolvedValue(vi.fn());
    await initializeDesktopBridge();
    const originalUrl = window.location.href;
    for (const hash of ["", "#/ko/settings", "#/ko/task/task-a", "#/ko/task/task-a"]) {
      window.history.replaceState(null, "", `${window.location.pathname}${hash}`);
      invoke.mockClear();
      const event = new KeyboardEvent("keydown", {
        key: "N", code: "KeyN", ctrlKey: true, shiftKey: true, cancelable: true,
      });
      window.dispatchEvent(event);
      expect(event.defaultPrevented).toBe(true);
      expect(invoke).toHaveBeenCalledExactlyOnceWith("desktop_native", {
        method: "createNewWindow", args: [hash.slice(1) || "/"],
      });
    }
    window.history.replaceState(null, "", originalUrl);
    invoke.mockClear();
    await window.kanvibeDesktop.openInternalRouteInNewWindow("/ko/task/task-a");
    expect(invoke).toHaveBeenCalledExactlyOnceWith("desktop_native", {
      method: "openInternalRouteInNewWindow", args: ["/ko/task/task-a"],
    });
  });

  it("구독 준비가 끝난 뒤 bridge를 노출하고 실제 payload와 구독 해제를 전달한다", async () => {
    listen.mockImplementation((name, callback) => {
      callbacks.set(name, callback);
      return new Promise(resolve => pending.push(() => resolve(vi.fn())));
    });
    delete (window as Partial<Window>).kanvibeDesktop;
    const ready = initializeDesktopBridge();
    expect(window.kanvibeDesktop).toBeUndefined();
    pending.forEach(resolve => resolve());
    await ready;
    const subscriber = vi.fn();
    const dispose = window.kanvibeDesktop.onTerminalData(subscriber);
    const payload = { taskId: "task", tabId: "tab", data: "한글" };
    callbacks.get("kanvibe:terminal-data")!({ payload });
    expect(subscriber).toHaveBeenCalledWith(payload);
    dispose();
    callbacks.get("kanvibe:terminal-data")!({ payload });
    expect(subscriber).toHaveBeenCalledTimes(1);
  });

  it("서비스 인자와 비동기 클립보드 결과를 Tauri 명령으로 전달한다", async () => {
    listen.mockResolvedValue(vi.fn());
    await initializeDesktopBridge();
    await window.kanvibeDesktop.invoke("kanban", "getTaskById", ["task"]);
    expect(invoke).toHaveBeenCalledWith("desktop_invoke", {
      namespace: "kanban", method: "getTaskById", args: ["task"],
    });
    invoke.mockResolvedValueOnce("data:image/png;base64,png");
    expect(await window.kanvibeDesktop.readClipboardImage()).toBe("data:image/png;base64,png");
    expect(invoke).toHaveBeenLastCalledWith("desktop_native", { method: "readClipboardImage", args: [] });
  });
});
