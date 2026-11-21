import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import DeleteTaskButton from "../DeleteTaskButton";

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, unknown>) => (
    values ? `${key}:${JSON.stringify(values)}` : key
  ),
}));

describe("DeleteTaskButton", () => {
  let confirmMock: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    confirmMock = vi.spyOn(window, "confirm").mockReturnValue(true);
  });

  afterEach(() => {
    confirmMock.mockRestore();
  });

  it("삭제 응답을 기다리는 동안 버튼을 막고 삭제 중 문구를 보인다", async () => {
    let resolveDelete: () => void = () => {};
    const deleteAction = vi.fn(() => new Promise<void>((resolve) => {
      resolveDelete = resolve;
    }));
    render(<DeleteTaskButton deleteAction={deleteAction} />);

    fireEvent.click(screen.getByRole("button", { name: "delete" }));

    const pendingButton = await screen.findByRole("button", { name: "deleting" });
    expect((pendingButton as HTMLButtonElement).disabled).toBe(true);

    fireEvent.click(pendingButton);
    expect(deleteAction).toHaveBeenCalledTimes(1);

    resolveDelete();
    await screen.findByRole("button", { name: "delete" });
  });

  it("삭제가 실패하면 사유를 alert로 보이고 버튼을 다시 쓸 수 있게 한다", async () => {
    const deleteAction = vi.fn().mockRejectedValue(new Error("worktree busy"));
    render(<DeleteTaskButton deleteAction={deleteAction} />);

    fireEvent.click(screen.getByRole("button", { name: "delete" }));

    await waitFor(() => {
      expect(screen.getByRole("alert").textContent).toBe('deleteFailed:{"reason":"worktree busy"}');
    });
    expect((screen.getByRole("button", { name: "delete" }) as HTMLButtonElement).disabled).toBe(false);
  });
});
