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
  sessionStorage.clear();
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

    it("should sync valid URL filter to sessionStorage on mount", () => {
      // Given
      mockGet.mockReturnValue("proj-1,proj-2");

      // When
      renderHook(() => useProjectFilterParams(VALID_IDS));

      // Then
      expect(sessionStorage.getItem("kanvibe_project_filter")).toBe("proj-1,proj-2");
    });
  });

  describe("sessionStorage restoration", () => {
    it("should restore filter from sessionStorage when URL has no projects param", () => {
      // Given
      mockGet.mockReturnValue(null);
      sessionStorage.setItem("kanvibe_project_filter", "proj-1,proj-3");

      // When
      const { result } = renderHook(() => useProjectFilterParams(VALID_IDS));

      // Then
      expect(result.current[0]).toEqual(["proj-1", "proj-3"]);
    });

    it("should update URL when restoring from sessionStorage", () => {
      // Given
      mockGet.mockReturnValue(null);
      sessionStorage.setItem("kanvibe_project_filter", "proj-2");

      // When
      renderHook(() => useProjectFilterParams(VALID_IDS));

      // Then
      expect(window.history.replaceState).toHaveBeenCalledWith(
        null,
        "",
        expect.stringContaining("projects=proj-2"),
      );
    });

    it("should filter out invalid IDs from sessionStorage", () => {
      // Given
      mockGet.mockReturnValue(null);
      sessionStorage.setItem("kanvibe_project_filter", "proj-1,deleted-proj,proj-2");

      // When
      const { result } = renderHook(() => useProjectFilterParams(VALID_IDS));

      // Then
      expect(result.current[0]).toEqual(["proj-1", "proj-2"]);
    });

    it("should return empty array when sessionStorage has only invalid IDs", () => {
      // Given
      mockGet.mockReturnValue(null);
      sessionStorage.setItem("kanvibe_project_filter", "deleted-1,deleted-2");

      // When
      const { result } = renderHook(() => useProjectFilterParams(VALID_IDS));

      // Then
      expect(result.current[0]).toEqual([]);
    });

    it("should return empty array when sessionStorage has empty string", () => {
      // Given
      mockGet.mockReturnValue(null);
      sessionStorage.setItem("kanvibe_project_filter", "");

      // When
      const { result } = renderHook(() => useProjectFilterParams(VALID_IDS));

      // Then
      expect(result.current[0]).toEqual([]);
    });

    it("should gracefully handle sessionStorage access error on restore", () => {
      // Given
      mockGet.mockReturnValue(null);
      const originalGetItem = sessionStorage.getItem;
      vi.spyOn(sessionStorage, "getItem").mockImplementation(() => {
        throw new Error("SecurityError");
      });

      // When
      const { result } = renderHook(() => useProjectFilterParams(VALID_IDS));

      // Then
      expect(result.current[0]).toEqual([]);
      sessionStorage.getItem = originalGetItem;
    });

    it("should restore only once across re-renders", () => {
      // Given
      mockGet.mockReturnValue(null);
      sessionStorage.setItem("kanvibe_project_filter", "proj-1");

      // When
      const { result, rerender } = renderHook(() => useProjectFilterParams(VALID_IDS));
      (window.history.replaceState as ReturnType<typeof vi.fn>).mockClear();
      rerender();

      // Then
      expect(result.current[0]).toEqual(["proj-1"]);
      expect(window.history.replaceState).not.toHaveBeenCalled();
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

    it("should sync to sessionStorage when setting IDs", () => {
      // Given
      mockGet.mockReturnValue(null);
      const { result } = renderHook(() => useProjectFilterParams(VALID_IDS));

      // When
      act(() => {
        result.current[1](["proj-1", "proj-3"]);
      });

      // Then
      expect(sessionStorage.getItem("kanvibe_project_filter")).toBe("proj-1,proj-3");
    });

    it("should remove sessionStorage entry when setting empty array", () => {
      // Given
      mockGet.mockReturnValue("proj-1");
      const { result } = renderHook(() => useProjectFilterParams(VALID_IDS));

      // When
      act(() => {
        result.current[1]([]);
      });

      // Then
      expect(sessionStorage.getItem("kanvibe_project_filter")).toBeNull();
    });

    it("should gracefully handle sessionStorage access error on sync", () => {
      // Given
      mockGet.mockReturnValue(null);
      const { result } = renderHook(() => useProjectFilterParams(VALID_IDS));
      vi.spyOn(sessionStorage, "setItem").mockImplementation(() => {
        throw new Error("QuotaExceededError");
      });

      // When & Then
      expect(() => {
        act(() => {
          result.current[1](["proj-1"]);
        });
      }).not.toThrow();
      expect(result.current[0]).toEqual(["proj-1"]);
    });
  });
});
