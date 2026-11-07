import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  settings: new Map<string, string>(),
  getAppSetting: vi.fn(),
  setAppSetting: vi.fn(),
}));

vi.mock("@/desktop/main/services/appSettingsService", () => ({
  getAppSetting: mocks.getAppSetting,
  setAppSetting: mocks.setAppSetting,
}));

describe("taskCliTokenService", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.settings.clear();
    mocks.getAppSetting.mockImplementation(async (key: string) => mocks.settings.get(key) ?? null);
    mocks.setAppSetting.mockImplementation(async (key: string, value: string) => {
      mocks.settings.set(key, value);
    });
  });

  it("같은 task에는 같은 토큰을, 다른 task에는 다른 토큰을 만든다", async () => {
    const { buildTaskCliToken } = await import("@/desktop/main/services/taskCliTokenService");

    const firstToken = await buildTaskCliToken("task-1");

    expect(firstToken).toMatch(/^[0-9a-f]{64}$/);
    expect(await buildTaskCliToken("task-1")).toBe(firstToken);
    expect(await buildTaskCliToken("task-2")).not.toBe(firstToken);
  });

  it("토큰은 그 토큰을 발급한 task에서만 통한다", async () => {
    const { buildTaskCliToken, isValidTaskCliToken } = await import("@/desktop/main/services/taskCliTokenService");
    const token = await buildTaskCliToken("task-1");

    expect(await isValidTaskCliToken("task-1", token)).toBe(true);
    expect(await isValidTaskCliToken("task-2", token)).toBe(false);
    expect(await isValidTaskCliToken("task-1", "forged")).toBe(false);
  });

  it("동시에 처음 발급해도 비밀값은 한 번만 저장하고, 앱을 다시 띄워도 같은 비밀값으로 검증한다", async () => {
    const firstRun = await import("@/desktop/main/services/taskCliTokenService");
    const [tokenA, tokenB] = await Promise.all([
      firstRun.buildTaskCliToken("task-1"),
      firstRun.buildTaskCliToken("task-1"),
    ]);

    vi.resetModules();
    const restartedRun = await import("@/desktop/main/services/taskCliTokenService");

    expect(tokenA).toBe(tokenB);
    expect(mocks.setAppSetting).toHaveBeenCalledTimes(1);
    expect(await restartedRun.isValidTaskCliToken("task-1", tokenA)).toBe(true);
  });
});
