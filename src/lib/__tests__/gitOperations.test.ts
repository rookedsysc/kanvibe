import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  homedir: vi.fn(() => "/home/local-user"),
}));

vi.mock("os", () => ({
  default: {
    homedir: mocks.homedir,
  },
  homedir: mocks.homedir,
}));

describe("gitOperations.resolvePathForShell", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  it("로컬 틸드 경로는 로컬 홈 디렉토리로 확장한다", async () => {
    // Given
    const { resolvePathForShell } = await import("@/lib/gitOperations");

    // When
    const result = resolvePathForShell("~/work");

    // Then
    expect(result).toBe('"/home/local-user/work"');
  });

  it("원격 틸드 경로는 원격 HOME 기준으로 검색한다", async () => {
    // Given
    const { resolvePathForShell } = await import("@/lib/gitOperations");

    // When
    const result = resolvePathForShell("~/work", "remote-host");

    // Then
    expect(result).toBe('"$HOME/work"');
  });
});
