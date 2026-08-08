import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  appSettingsRepo: {
    create: vi.fn(),
    findOne: vi.fn(),
    save: vi.fn(),
  },
}));

vi.mock("@/lib/database", () => ({
  getAppSettingsRepository: vi.fn(async () => mocks.appSettingsRepo),
}));

vi.mock("@/entities/KanbanTask", () => ({
  SessionType: {
    TMUX: "tmux",
    ZELLIJ: "zellij",
  },
}));

const BOARD_SORT_PREFERENCE_KEY = "board_sort_preference";

describe("보드 정렬 설정", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.appSettingsRepo.create.mockImplementation((value) => value);
    mocks.appSettingsRepo.save.mockResolvedValue(undefined);
    mocks.appSettingsRepo.findOne.mockResolvedValue(null);
  });

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
    const [savedSetting] = mocks.appSettingsRepo.save.mock.calls[0];
    mocks.appSettingsRepo.findOne.mockResolvedValue(savedSetting);
    const restored = await getBoardSortPreference();

    // Then
    expect(savedSetting.key).toBe(BOARD_SORT_PREFERENCE_KEY);
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
    mocks.appSettingsRepo.findOne.mockResolvedValue({
      key: BOARD_SORT_PREFERENCE_KEY,
      value: "{ 깨진 값",
    });
    const { getBoardSortPreference } = await import("@/desktop/main/services/appSettingsService");

    // When
    const preference = await getBoardSortPreference();

    // Then
    expect(preference).toEqual({ keys: [] });
  });
});
