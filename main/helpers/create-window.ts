import { BrowserWindow, type BrowserWindowConstructorOptions, screen } from "electron";
import Store from "electron-store";

/**
 * 윈도우 위치/크기를 electron-store에 저장하고 복원한다.
 * 앱 재시작 시 마지막 윈도우 상태를 유지한다.
 */
const store = new Store<Record<string, { x: number; y: number; width: number; height: number }>>({
  name: "window-state",
});

export function createWindow(
  name: string,
  options: BrowserWindowConstructorOptions,
): BrowserWindow {
  const savedState = store.get(name);
  const defaultSize = {
    width: options.width || 1400,
    height: options.height || 900,
  };

  const windowOptions: BrowserWindowConstructorOptions = {
    ...options,
    ...defaultSize,
    ...savedState,
  };

  /** 저장된 위치가 현재 디스플레이 밖이면 기본 위치로 보정한다 */
  if (savedState) {
    const displays = screen.getAllDisplays();
    const isVisible = displays.some((display) => {
      const { x, y, width, height } = display.bounds;
      return (
        savedState.x >= x &&
        savedState.y >= y &&
        savedState.x < x + width &&
        savedState.y < y + height
      );
    });

    if (!isVisible) {
      delete windowOptions.x;
      delete windowOptions.y;
    }
  }

  const win = new BrowserWindow(windowOptions);

  /** 윈도우 이동/리사이즈 시 상태를 저장한다 */
  const saveState = () => {
    if (win.isMinimized() || win.isMaximized()) return;
    const bounds = win.getBounds();
    store.set(name, bounds);
  };

  win.on("resize", saveState);
  win.on("move", saveState);

  return win;
}
