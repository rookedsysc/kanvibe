import { describe, it, expect, vi, beforeEach } from "vitest";

const { mockInvokeDesktop, mockTriggerDesktopRefresh } = vi.hoisted(() => ({
  mockInvokeDesktop: vi.fn(),
  mockTriggerDesktopRefresh: vi.fn(),
}));

vi.mock("@/desktop/renderer/ipc", () => ({
  invokeDesktop: mockInvokeDesktop,
}));

vi.mock("@/desktop/renderer/utils/refresh", () => ({
  triggerDesktopRefresh: mockTriggerDesktopRefresh,
}));

describe("project actions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockInvokeDesktop.mockResolvedValue({ success: true });
  });

  it("does not trigger a global refresh for task hook installs", async () => {
    const { installTaskHooks } = await import("@/desktop/renderer/actions/project");

    await installTaskHooks("task-1");

    expect(mockInvokeDesktop).toHaveBeenCalledWith("project", "installTaskHooks", "task-1");
    expect(mockTriggerDesktopRefresh).not.toHaveBeenCalled();
  });

  it("still triggers a global refresh for project registration", async () => {
    const { registerProject } = await import("@/desktop/renderer/actions/project");

    await registerProject("kanvibe", "/repo");

    expect(mockInvokeDesktop).toHaveBeenCalledWith("project", "registerProject", "kanvibe", "/repo", undefined);
    expect(mockTriggerDesktopRefresh).toHaveBeenCalledTimes(1);
  });
});
