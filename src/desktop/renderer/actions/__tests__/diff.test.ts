import { beforeEach, describe, expect, it, vi } from "vitest";

const { mockInvokeDesktop } = vi.hoisted(() => ({
  mockInvokeDesktop: vi.fn(),
}));

vi.mock("@/desktop/renderer/ipc", () => ({
  invokeDesktop: mockInvokeDesktop,
}));

describe("readTaskWorkspaceFile", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("읽은 파일을 성공 결과로 돌려준다", async () => {
    mockInvokeDesktop.mockResolvedValue({ path: "docs/plan.md", content: "# plan" });
    const { readTaskWorkspaceFile } = await import("@/desktop/renderer/actions/diff");

    await expect(readTaskWorkspaceFile("task-1", "/work/task/docs/plan.md"))
      .resolves.toEqual({ ok: true, file: { path: "docs/plan.md", content: "# plan" } });
    expect(mockInvokeDesktop).toHaveBeenCalledWith("diff", "readTaskWorkspaceFile", "task-1", "/work/task/docs/plan.md");
  });

  it.each(["outside-worktree", "not-found", "too-large", "unreadable"])("IPC를 건너온 오류 메시지에서 사유 %s를 읽는다", async (reason) => {
    mockInvokeDesktop.mockRejectedValue(new Error(`Error invoking remote method 'kanvibe:invoke': TaskWorkspaceFileError: ${reason}`));
    const { readTaskWorkspaceFile } = await import("@/desktop/renderer/actions/diff");

    await expect(readTaskWorkspaceFile("task-1", "a.md")).resolves.toEqual({ ok: false, reason });
  });

  it("사유를 읽을 수 없는 실패는 unreadable로 돌려준다", async () => {
    mockInvokeDesktop.mockRejectedValue(new Error("Error invoking remote method 'kanvibe:invoke': Error: Unknown desktop service method: diff.readTaskWorkspaceFile"));
    const { readTaskWorkspaceFile } = await import("@/desktop/renderer/actions/diff");

    await expect(readTaskWorkspaceFile("task-1", "a.md")).resolves.toEqual({ ok: false, reason: "unreadable" });
  });
});
