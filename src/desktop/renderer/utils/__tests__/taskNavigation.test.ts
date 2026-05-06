import { beforeEach, describe, expect, it, vi } from "vitest";
import { navigateToTaskDetail } from "@/desktop/renderer/utils/taskNavigation";

describe("navigateToTaskDetail", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.history.replaceState({}, "", "/#/en");
  });

  it("focuses an existing task detail window before falling back to in-window navigation", async () => {
    const navigate = vi.fn();
    const focusExistingInternalRoute = vi.fn().mockResolvedValue(true);
    window.kanvibeDesktop = {
      isDesktop: true,
      focusExistingInternalRoute,
    } as unknown as NonNullable<typeof window.kanvibeDesktop>;

    await navigateToTaskDetail("task-1", {
      currentLocale: "en",
      navigate,
    });

    expect(focusExistingInternalRoute).toHaveBeenCalledWith("/en/task/task-1");
    expect(navigate).not.toHaveBeenCalled();
  });

  it("navigates in the current window when no existing task detail window is focused", async () => {
    const navigate = vi.fn();
    const focusExistingInternalRoute = vi.fn().mockResolvedValue(false);
    window.kanvibeDesktop = {
      isDesktop: true,
      focusExistingInternalRoute,
    } as unknown as NonNullable<typeof window.kanvibeDesktop>;

    await navigateToTaskDetail("task-1", {
      currentLocale: "en",
      navigate,
    });

    expect(focusExistingInternalRoute).toHaveBeenCalledWith("/en/task/task-1");
    expect(navigate).toHaveBeenCalledWith("/en/task/task-1");
  });
});
