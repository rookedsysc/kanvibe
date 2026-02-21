// @vitest-environment node
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SessionType } from "@/entities/KanbanTask";

const mockFindOne = vi.fn();
const mockSave = vi.fn();
const mockCreate = vi.fn((value: Record<string, unknown>) => value);
const mockRevalidatePath = vi.fn();

vi.mock("@/lib/database", () => ({
  getAppSettingsRepository: vi.fn().mockResolvedValue({
    findOne: mockFindOne,
    save: mockSave,
    create: mockCreate,
  }),
}));

vi.mock("next/cache", () => ({
  revalidatePath: mockRevalidatePath,
}));

describe("appSettings default session type", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.resetModules();
  });

  it("기본 세션 타입 설정이 없으면 tmux를 반환한다", async () => {
    // Given
    mockFindOne.mockResolvedValue(null);
    const { getDefaultSessionType } = await import("@/app/actions/appSettings");

    // When
    const result = await getDefaultSessionType();

    // Then
    expect(result).toBe(SessionType.TMUX);
  });

  it("저장된 값이 zellij면 zellij를 반환한다", async () => {
    // Given
    mockFindOne.mockResolvedValue({ key: "default_session_type", value: SessionType.ZELLIJ });
    const { getDefaultSessionType } = await import("@/app/actions/appSettings");

    // When
    const result = await getDefaultSessionType();

    // Then
    expect(result).toBe(SessionType.ZELLIJ);
  });

  it("저장된 값이 예상 범위를 벗어나면 tmux로 폴백한다", async () => {
    // Given
    mockFindOne.mockResolvedValue({ key: "default_session_type", value: "invalid" });
    const { getDefaultSessionType } = await import("@/app/actions/appSettings");

    // When
    const result = await getDefaultSessionType();

    // Then
    expect(result).toBe(SessionType.TMUX);
  });

  it("기본 세션 타입 저장 시 값을 저장하고 경로를 갱신한다", async () => {
    // Given
    mockFindOne.mockResolvedValue(null);
    const { setDefaultSessionType } = await import("@/app/actions/appSettings");

    // When
    await setDefaultSessionType(SessionType.ZELLIJ);

    // Then
    expect(mockCreate).toHaveBeenCalledWith({
      key: "default_session_type",
      value: SessionType.ZELLIJ,
    });
    expect(mockSave).toHaveBeenCalledWith({
      key: "default_session_type",
      value: SessionType.ZELLIJ,
    });
    expect(mockRevalidatePath).toHaveBeenCalledWith("/");
  });
});
