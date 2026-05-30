import { describe, expect, it, vi } from "vitest";

vi.mock("@/desktop/renderer/ipc", () => ({
  invokeDesktop: vi.fn(),
}));

describe("renderer kanban actions", () => {
  it("does not expose task resource cleanup escape hatches to the renderer", async () => {
    const actions = await import("@/desktop/renderer/actions/kanban");

    expect(actions).not.toHaveProperty("cleanupTaskResources");
    expect(actions).not.toHaveProperty("deleteTaskResources");
    expect(actions).not.toHaveProperty("removeWorktreeAndSession");
    expect(actions).not.toHaveProperty("removeWorktreeAndBranch");
    expect(actions).not.toHaveProperty("removeSessionOnly");
  });
});
