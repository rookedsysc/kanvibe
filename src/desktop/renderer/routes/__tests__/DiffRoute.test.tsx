import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import DiffRoute from "@/desktop/renderer/routes/DiffRoute";
import { INITIAL_DESKTOP_LOAD_TIMEOUT_MS } from "@/desktop/renderer/utils/loadingTimeout";

const mocks = vi.hoisted(() => ({
  getTaskById: vi.fn(),
  getGitDiffFiles: vi.fn(),
}));

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, unknown>) => (
    values ? `${key}:${JSON.stringify(values)}` : key
  ),
}));

vi.mock("react-router-dom", () => ({
  useParams: () => ({ id: "task-1" }),
}));

vi.mock("@/desktop/renderer/navigation", () => ({
  Link: ({ children, href }: { children: React.ReactNode; href: string }) => <a href={href}>{children}</a>,
  usePathname: () => "/en/task/task-1/diff",
  useRouter: () => ({ back: vi.fn(), push: vi.fn() }),
}));

vi.mock("@/desktop/renderer/actions/kanban", () => ({
  getTaskById: (...args: unknown[]) => mocks.getTaskById(...args),
}));

vi.mock("@/desktop/renderer/actions/diff", () => ({
  getGitDiffFiles: (...args: unknown[]) => mocks.getGitDiffFiles(...args),
}));

vi.mock("@/desktop/renderer/components/DiffPageClient", () => ({
  default: ({ files }: { files: unknown[] }) => <div data-testid="diff-page">{files.length}</div>,
}));

vi.mock("@/desktop/renderer/utils/refresh", () => ({
  useRefreshSignal: () => 0,
}));

const taskWithWorktree = {
  id: "task-1",
  branchName: "feature/diff",
  worktreePath: "/repo__worktrees/feature-diff",
  baseBranch: "main",
};

describe("DiffRoute", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getTaskById.mockReset();
    mocks.getGitDiffFiles.mockResolvedValue([{ path: "a.ts" }]);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("조회가 오래 걸리면 task 없음 화면 대신 지연 안내를 띄운 채 기다린다", async () => {
    // Given
    vi.useFakeTimers();
    mocks.getTaskById.mockReturnValue(new Promise(() => {}));

    // When
    render(<DiffRoute />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(INITIAL_DESKTOP_LOAD_TIMEOUT_MS);
    });

    // Then
    expect(screen.getByRole("status").textContent).toBe("loadDelayed");
    expect(screen.queryByText("taskNotFound")).toBeNull();
  });

  it("조회가 실패하면 task 없음 화면 대신 실패 사유와 재시도를 보이고 재시도로 다시 불러온다", async () => {
    // Given
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.getTaskById
      .mockRejectedValueOnce(new Error("git timeout"))
      .mockResolvedValueOnce(taskWithWorktree);

    try {
      // When
      render(<DiffRoute />);

      // Then
      const alert = await screen.findByRole("alert");
      expect(alert.textContent).toContain('loadFailed:{"reason":"git timeout"}');
      expect(screen.queryByText("taskNotFound")).toBeNull();

      fireEvent.click(screen.getByRole("button", { name: "retry" }));

      await waitFor(() => {
        expect(screen.getByTestId("diff-page").textContent).toBe("1");
      });
    } finally {
      consoleError.mockRestore();
    }
  });

  it("조회는 성공했지만 task가 없으면 task 없음 화면을 보인다", async () => {
    // Given
    mocks.getTaskById.mockResolvedValue(null);

    // When
    render(<DiffRoute />);

    // Then
    expect(await screen.findByText("taskNotFound")).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
