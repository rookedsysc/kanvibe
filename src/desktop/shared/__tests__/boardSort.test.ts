import { describe, expect, it } from "vitest";
import {
  DEFAULT_BOARD_SORT_PREFERENCE,
  parseBoardSortPreference,
  serializeBoardSortPreference,
} from "@/desktop/shared/boardSort";

describe("parseBoardSortPreference", () => {
  it("저장했던 기준과 모드를 그대로 돌려준다", () => {
    // Given
    const preference = {
      keys: [
        { field: "priority" as const, direction: "asc" as const },
        { field: "title" as const, direction: "desc" as const },
      ],
      mode: "rank-first" as const,
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
      mode: "whatever",
    });

    // When
    const restored = parseBoardSortPreference(stored);

    // Then
    expect(restored.keys).toEqual([{ field: "createdAt", direction: "asc" }]);
    expect(restored.mode).toBe(DEFAULT_BOARD_SORT_PREFERENCE.mode);
  });

  it("없어진 모드 이름은 지금 어휘로 옮겨 읽는다", () => {
    // Given
    /** 이전 버전이 저장해 둔 설정을 그대로 들고 올라오는 경우 */
    const storedManualFirst = JSON.stringify({ keys: [], mode: "manual-first" });
    const storedManualOff = JSON.stringify({ keys: [], mode: "manual-off" });

    // When / Then
    /** "수동 순서 우선"은 드래그한 자리를 먼저 보겠다는 뜻이었다 */
    expect(parseBoardSortPreference(storedManualFirst).mode).toBe("rank-first");
    /** "수동 순서 사용 안 함"은 정렬 기준만 보겠다는 뜻이었다 */
    expect(parseBoardSortPreference(storedManualOff).mode).toBe("sort-first");
  });

  it("같은 기준이 두 번 들어 있으면 앞의 것만 남긴다", () => {
    // Given
    const stored = JSON.stringify({
      keys: [
        { field: "title", direction: "desc" },
        { field: "title", direction: "asc" },
      ],
      mode: "sort-first",
    });

    // When
    const restored = parseBoardSortPreference(stored);

    // Then
    expect(restored.keys).toEqual([{ field: "title", direction: "desc" }]);
  });
});
