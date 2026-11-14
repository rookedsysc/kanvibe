import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
import ProjectColorEditor from "../ProjectColorEditor";

const mockRefresh = vi.fn();
const mockUpdateProjectColor = vi.fn();

vi.mock("@/desktop/renderer/navigation", () => ({
  useRouter: () => ({ refresh: mockRefresh }),
}));

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, unknown>) => (
    values ? `${key}:${JSON.stringify(values)}` : key
  ),
}));

vi.mock("@/desktop/renderer/actions/kanban", () => ({
  updateProjectColor: (...args: unknown[]) => mockUpdateProjectColor(...args),
}));

describe("ProjectColorEditor - 저장 상태", () => {
  beforeEach(() => {
    mockRefresh.mockClear();
    mockUpdateProjectColor.mockReset();
  });

  it("색 저장이 실패하면 미리 바꾼 색을 저장된 색으로 되돌리고 사유를 알린다", async () => {
    mockUpdateProjectColor.mockRejectedValueOnce(new Error("db locked"));
    render(<ProjectColorEditor projectId="project-1" projectName="kanvibe" currentColor="#93C5FD" />);

    fireEvent.click(screen.getByRole("button", { name: "Pick color" }));
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Color #F9A8D4" }));
    });

    expect(screen.getByRole("alert").textContent).toBe('saveFailed:{"reason":"db locked"}');
    expect((screen.getByRole("button", { name: "Pick color" }) as HTMLButtonElement).style.backgroundColor).toBe("rgb(147, 197, 253)");
    expect(mockRefresh).not.toHaveBeenCalled();
  });

  it("저장하는 동안에는 색 선택을 다시 열지 못한다", async () => {
    let resolveSave: () => void = () => {};
    mockUpdateProjectColor.mockReturnValueOnce(new Promise<void>((resolve) => {
      resolveSave = resolve;
    }));
    render(<ProjectColorEditor projectId="project-1" projectName="kanvibe" currentColor="#93C5FD" />);

    fireEvent.click(screen.getByRole("button", { name: "Pick color" }));
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Color #F9A8D4" }));
    });

    expect((screen.getByRole("button", { name: "Pick color" }) as HTMLButtonElement).disabled).toBe(true);

    await act(async () => {
      resolveSave();
    });
    expect(mockRefresh).toHaveBeenCalledTimes(1);
  });
});
