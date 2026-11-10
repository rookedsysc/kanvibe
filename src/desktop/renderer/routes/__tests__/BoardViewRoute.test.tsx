import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import BoardViewRoute from "@/desktop/renderer/routes/BoardViewRoute";

const mocks = vi.hoisted(() => ({
  getAppSetting: vi.fn(),
}));

vi.mock("@/desktop/renderer/ipc", () => ({
  invokeDesktop: (_service: string, method: string, ...args: unknown[]) => (
    method === "getAppSetting" ? mocks.getAppSetting(...args) : Promise.resolve(undefined)
  ),
}));

vi.mock("@/desktop/renderer/routes/BoardRoute", () => ({
  default: () => <div data-testid="kanban-view" />,
}));

vi.mock("@/desktop/renderer/routes/TaskWorkspaceRoute", () => ({
  default: () => <div data-testid="workspace-view" />,
}));

describe("BoardViewRoute", () => {
  beforeEach(() => {
    mocks.getAppSetting.mockReset();
  });

  it("저장된 보드 뷰가 workspace면 워크스페이스를 보여 준다", async () => {
    // Given
    mocks.getAppSetting.mockResolvedValue("workspace");

    // When
    render(<BoardViewRoute />);

    // Then
    expect(await screen.findByTestId("workspace-view")).toBeTruthy();
    expect(screen.queryByTestId("kanban-view")).toBeNull();
    expect(mocks.getAppSetting).toHaveBeenCalledWith("board_view_mode");
  });

  it.each([
    ["kanban", "kanban"],
    ["저장값 없음", null],
    ["알 수 없는 값", "timeline"],
  ])("저장된 보드 뷰가 %s이면 칸반을 보여 준다", async (_label, storedValue) => {
    // Given
    mocks.getAppSetting.mockResolvedValue(storedValue);

    // When
    render(<BoardViewRoute />);

    // Then
    expect(await screen.findByTestId("kanban-view")).toBeTruthy();
    expect(screen.queryByTestId("workspace-view")).toBeNull();
  });

  it("보드 뷰 설정을 읽지 못하면 칸반을 보여 준다", async () => {
    // Given
    mocks.getAppSetting.mockRejectedValue(new Error("ipc down"));
    vi.spyOn(console, "error").mockImplementation(() => {});

    // When
    render(<BoardViewRoute />);

    // Then
    await waitFor(() => expect(screen.getByTestId("kanban-view")).toBeTruthy());
  });
});
