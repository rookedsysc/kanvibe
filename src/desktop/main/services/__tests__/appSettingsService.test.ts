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

describe("알림 안읽음만 보기 설정", () => {
  it("켠 상태를 앱을 다시 켠 뒤에도 그대로 읽는다", async () => {
    // Given
    const { getNotificationUnreadOnlyEnabled, setNotificationUnreadOnlyEnabled } = await import(
      "@/desktop/main/services/appSettingsService"
    );

    // When
    await setNotificationUnreadOnlyEnabled(true);

    // Then
    expect(await getNotificationUnreadOnlyEnabled()).toBe(true);
  });

  it("껐다면 전체 보기로 되돌린다", async () => {
    // Given
    const { getNotificationUnreadOnlyEnabled, setNotificationUnreadOnlyEnabled } = await import(
      "@/desktop/main/services/appSettingsService"
    );
    await setNotificationUnreadOnlyEnabled(true);

    // When
    await setNotificationUnreadOnlyEnabled(false);

    // Then
    expect(await getNotificationUnreadOnlyEnabled()).toBe(false);
  });

  it("한 번도 저장한 적이 없으면 전체 보기로 시작한다", async () => {
    // Given
    const { getNotificationUnreadOnlyEnabled } = await import("@/desktop/main/services/appSettingsService");

    // When
    const isUnreadOnly = await getNotificationUnreadOnlyEnabled();

    // Then
    expect(isUnreadOnly).toBe(false);
  });
});

const BOARD_SORT_PREFERENCE_KEY = "board_sort_preference";

describe("보드 정렬 설정", () => {
  it("저장한 기준을 앱을 다시 켠 뒤에도 그대로 읽는다", async () => {
    // Given
    const preference = {
      keys: [{ field: "priority" as const, direction: "desc" as const }],
    };
    const { getBoardSortPreference, setBoardSortPreference } = await import(
      "@/desktop/main/services/appSettingsService"
    );

    // When
    await setBoardSortPreference(preference);
    const restored = await getBoardSortPreference();

    // Then
    expect(storedSettings.has(BOARD_SORT_PREFERENCE_KEY)).toBe(true);
    expect(restored).toEqual(preference);
  });

  it("한 번도 저장한 적이 없으면 기준 없이 시작한다", async () => {
    // Given
    const { getBoardSortPreference } = await import("@/desktop/main/services/appSettingsService");

    // When
    const preference = await getBoardSortPreference();

    // Then
    expect(preference).toEqual({ keys: [] });
  });

  it("저장된 값이 깨져 있어도 보드를 못 그리는 대신 기본값으로 되돌린다", async () => {
    // Given
    storedSettings.set(BOARD_SORT_PREFERENCE_KEY, "{ 깨진 값");
    const { getBoardSortPreference } = await import("@/desktop/main/services/appSettingsService");

    // When
    const preference = await getBoardSortPreference();

    // Then
    expect(preference).toEqual({ keys: [] });
  });
});
