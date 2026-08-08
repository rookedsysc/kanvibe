import { describe, expect, it } from "vitest";
import {
  DEFAULT_BOARD_SORT_PREFERENCE,
  parseBoardSortPreference,
  serializeBoardSortPreference,
} from "@/desktop/shared/boardSort";

describe("parseBoardSortPreference", () => {
  it("저장했던 기준을 그대로 돌려준다", () => {
    // Given
    const preference = {
      keys: [
        { field: "priority" as const, direction: "asc" as const },
        { field: "title" as const, direction: "desc" as const },
      ],
    };

    // When
    const restored = parseBoardSortPreference(serializeBoardSortPreference(preference));

    // Then
    expect(restored).toEqual(preference);
  });

  it("저장된 값이 없으면 기본값을 준다", () => {
    // Given / When / Then
    expect(parseBoardSortPreference(null)).toEqual(DEFAULT_BOARD_SORT_PREFERENCE);
  });

  it("깨진 JSON이면 기본값으로 되돌린다", () => {
    // Given / When / Then
    expect(parseBoardSortPreference("{ not json")).toEqual(DEFAULT_BOARD_SORT_PREFERENCE);
  });

  it("알아볼 수 없는 기준과 모드는 버리고 나머지는 살린다", () => {
    // Given
    const stored = JSON.stringify({
      keys: [
        { field: "unknownField", direction: "asc" },
        { field: "createdAt", direction: "sideways" },
      ],
    });

    // When
    const restored = parseBoardSortPreference(stored);

    // Then
    expect(restored.keys).toEqual([{ field: "createdAt", direction: "asc" }]);
  });

  it("수동 배치를 걷어내기 전에 저장된 mode 필드는 그대로 버린다", () => {
    // Given
    /** 이전 버전이 저장해 둔 설정을 그대로 들고 올라오는 경우 */
    const stored = JSON.stringify({ keys: [{ field: "title", direction: "asc" }], mode: "manual-first" });

    // When
    const restored = parseBoardSortPreference(stored);

    // Then
    expect(restored).toEqual({ keys: [{ field: "title", direction: "asc" }] });
  });

  it("같은 기준이 두 번 들어 있으면 앞의 것만 남긴다", () => {
    // Given
    const stored = JSON.stringify({
      keys: [
        { field: "title", direction: "desc" },
        { field: "title", direction: "asc" },
      ],
    });

    // When
    const restored = parseBoardSortPreference(stored);

    // Then
    expect(restored.keys).toEqual([{ field: "title", direction: "desc" }]);
  });
});
