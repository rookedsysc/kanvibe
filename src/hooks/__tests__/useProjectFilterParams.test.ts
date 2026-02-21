import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act, cleanup } from "@testing-library/react";

const mockGet = vi.fn();

vi.mock("next/navigation", () => ({
  useSearchParams: () => ({
    get: mockGet,
  }),
}));

import { useProjectFilterParams } from "../useProjectFilterParams";

const VALID_IDS = ["proj-1", "proj-2", "proj-3"];

beforeEach(() => {
  mockGet.mockReset();
  vi.stubGlobal("location", { href: "http://localhost:3000/ko" });
  vi.stubGlobal("queueMicrotask", (fn: () => void) => fn());
  vi.spyOn(window.history, "replaceState").mockImplementation(() => {});
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("useProjectFilterParams", () => {
  describe("initialization from URL", () => {
    it("should return empty array when no projects param exists", () => {
      // Given
      mockGet.mockReturnValue(null);

      // When
      const { result } = renderHook(() => useProjectFilterParams(VALID_IDS));

      // Then
      expect(result.current[0]).toEqual([]);
    });

    it("should parse comma-separated project IDs from URL", () => {
      // Given
      mockGet.mockReturnValue("proj-1,proj-3");

      // When
      const { result } = renderHook(() => useProjectFilterParams(VALID_IDS));

      // Then
      expect(result.current[0]).toEqual(["proj-1", "proj-3"]);
    });

    it("should filter out invalid project IDs", () => {
      // Given
      mockGet.mockReturnValue("proj-1,invalid-id,proj-2");

      // When
      const { result } = renderHook(() => useProjectFilterParams(VALID_IDS));

      // Then
      expect(result.current[0]).toEqual(["proj-1", "proj-2"]);
    });

    it("should return empty array when all IDs are invalid", () => {
      // Given
      mockGet.mockReturnValue("invalid-1,invalid-2");

      // When
      const { result } = renderHook(() => useProjectFilterParams(VALID_IDS));

      // Then
      expect(result.current[0]).toEqual([]);
    });
  });

  describe("URL synchronization on state change", () => {
    it("should update URL with projects param when setting IDs", () => {
      // Given
      mockGet.mockReturnValue(null);
      const { result } = renderHook(() => useProjectFilterParams(VALID_IDS));

      // When
      act(() => {
        result.current[1](["proj-1", "proj-2"]);
      });

      // Then
      expect(result.current[0]).toEqual(["proj-1", "proj-2"]);
      expect(window.history.replaceState).toHaveBeenCalledWith(
        null,
        "",
        expect.stringContaining("projects=proj-1%2Cproj-2"),
      );
    });

    it("should remove projects param from URL when setting empty array", () => {
      // Given
      mockGet.mockReturnValue("proj-1");
      const { result } = renderHook(() => useProjectFilterParams(VALID_IDS));

      // When
      act(() => {
        result.current[1]([]);
      });

      // Then
      expect(result.current[0]).toEqual([]);
      const calledUrl = (window.history.replaceState as ReturnType<typeof vi.fn>).mock.calls.at(-1)?.[2] as string;
      expect(new URL(calledUrl).searchParams.has("projects")).toBe(false);
    });

    it("should support updater function pattern", () => {
      // Given
      mockGet.mockReturnValue("proj-1");
      const { result } = renderHook(() => useProjectFilterParams(VALID_IDS));

      // When
      act(() => {
        result.current[1]((prev) => [...prev, "proj-2"]);
      });

      // Then
      expect(result.current[0]).toEqual(["proj-1", "proj-2"]);
    });
  });
});
