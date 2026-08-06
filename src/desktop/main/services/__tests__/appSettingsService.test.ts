/**
 * @vitest-environment node
 */
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SessionType } from "@/entities/KanbanTask";

const storedSettings = new Map<string, string>();

vi.mock("@/lib/database", () => ({
  getAppSettingsRepository: async () => ({
    findOne: async ({ where }: { where: { key: string } }) => (
      storedSettings.has(where.key) ? { key: where.key, value: storedSettings.get(where.key) } : null
    ),
    create: (setting: { key: string; value: string }) => setting,
    save: async (setting: { key: string; value: string }) => {
      storedSettings.set(setting.key, setting.value);
      return setting;
    },
  }),
}));

beforeEach(() => {
  storedSettings.clear();
});

describe("기본 세션 타입", () => {
  it("저장한 세 가지 세션 타입을 그대로 되돌려준다", async () => {
    const { getDefaultSessionType, setDefaultSessionType } = await import(
      "@/desktop/main/services/appSettingsService"
    );

    for (const sessionType of Object.values(SessionType)) {
      await setDefaultSessionType(sessionType);
      expect(await getDefaultSessionType()).toBe(sessionType);
    }
  });

  it("미설정이거나 모르는 값이면 tmux로 되돌린다", async () => {
    const { getDefaultSessionType } = await import("@/desktop/main/services/appSettingsService");

    expect(await getDefaultSessionType()).toBe(SessionType.TMUX);

    storedSettings.set("default_session_type", "screen");
    expect(await getDefaultSessionType()).toBe(SessionType.TMUX);
  });
});
