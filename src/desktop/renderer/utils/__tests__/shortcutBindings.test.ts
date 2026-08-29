import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_SHORTCUT_BINDINGS } from "@/desktop/shared/shortcutBindings";

const mocks = vi.hoisted(() => ({
  getShortcutBindings: vi.fn(),
  setShortcutBindings: vi.fn(),
}));

vi.mock("@/desktop/renderer/actions/appSettings", () => ({
  getShortcutBindings: (...args: unknown[]) => mocks.getShortcutBindings(...args),
  setShortcutBindings: (...args: unknown[]) => mocks.setShortcutBindings(...args),
}));

/** 캐시는 모듈 전역이라, 테스트마다 새로 불러와야 앞 테스트가 채운 값을 보지 않는다 */
function importShortcutBindings() {
  vi.resetModules();
  return import("@/desktop/renderer/utils/shortcutBindings");
}

describe("loadShortcutBindings", () => {
  let consoleErrorSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    vi.clearAllMocks();
    consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    consoleErrorSpy.mockRestore();
  });

  /**
   * 저장은 단축키 표를 통째로 치환한다. 조회 실패를 성공과 구분하지 못하면 기본값 표가 그대로 저장돼
   * 저장돼 있던 다른 명령의 재배정이 전부 지워진다.
   */
  it("조회에 실패하면 캐시를 채우지 않고 실패를 알린다", async () => {
    mocks.getShortcutBindings.mockRejectedValue(new Error("설정 조회 실패"));
    const { hasLoadedShortcutBindings, loadShortcutBindings, readShortcutBindings } = await importShortcutBindings();

    expect(await loadShortcutBindings()).toBe(false);
    expect(hasLoadedShortcutBindings()).toBe(false);
    expect(readShortcutBindings()).toEqual(DEFAULT_SHORTCUT_BINDINGS);
  });

  it("조회에 성공하면 저장된 표를 캐시에 담는다", async () => {
    const storedBindings = { ...DEFAULT_SHORTCUT_BINDINGS, taskSearch: "Mod+J" };
    mocks.getShortcutBindings.mockResolvedValue(storedBindings);
    const { hasLoadedShortcutBindings, loadShortcutBindings, readShortcutBindings } = await importShortcutBindings();

    expect(await loadShortcutBindings()).toBe(true);
    expect(hasLoadedShortcutBindings()).toBe(true);
    expect(readShortcutBindings()).toEqual(storedBindings);
  });

  /** 다시 읽기에 실패한 캐시는 다른 창이 바꾼 값을 놓친 옛 표라, 그대로 저장하면 그 변경을 지운다 */
  it("한 번 성공한 뒤 다시 읽기에 실패하면 캐시를 담고 있다고 하지 않는다", async () => {
    mocks.getShortcutBindings.mockResolvedValueOnce({ ...DEFAULT_SHORTCUT_BINDINGS });
    const { hasLoadedShortcutBindings, loadShortcutBindings } = await importShortcutBindings();
    expect(await loadShortcutBindings()).toBe(true);

    mocks.getShortcutBindings.mockRejectedValueOnce(new Error("설정 조회 실패"));

    expect(await loadShortcutBindings()).toBe(false);
    expect(hasLoadedShortcutBindings()).toBe(false);
  });
});
