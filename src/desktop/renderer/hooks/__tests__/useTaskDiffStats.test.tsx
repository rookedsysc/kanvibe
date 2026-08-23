import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import { useBoardTaskDiffStats } from "@/desktop/renderer/hooks/useTaskDiffStats";

const mocks = vi.hoisted(() => ({
  getTaskDiffStats: vi.fn(),
  getGitDiffFiles: vi.fn(),
}));

vi.mock("@/desktop/renderer/actions/diff", () => ({
  getTaskDiffStats: (...args: unknown[]) => mocks.getTaskDiffStats(...args),
  getGitDiffFiles: (...args: unknown[]) => mocks.getGitDiffFiles(...args),
}));

function BoardDiffStatsProbe({ taskIds, isEnabled }: { taskIds: string[]; isEnabled: boolean }) {
  const statsByTaskId = useBoardTaskDiffStats(taskIds, isEnabled);

  return <span data-testid="additions">{statsByTaskId["task-1"]?.additions ?? "none"}</span>;
}

describe("useBoardTaskDiffStats", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    mocks.getTaskDiffStats.mockResolvedValue({
      "task-1": { fileCount: 2, additions: 340, deletions: 76 },
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("보드에 떠 있는 태스크들을 한 번의 조회로 읽는다", async () => {
    render(<BoardDiffStatsProbe taskIds={["task-1", "task-2"]} isEnabled />);

    await waitFor(() => {
      expect(screen.getByTestId("additions").textContent).toBe("340");
    });
    expect(mocks.getTaskDiffStats).toHaveBeenCalledTimes(1);
    expect(mocks.getTaskDiffStats).toHaveBeenCalledWith(["task-1", "task-2"]);
  });

  it("주기가 돌 때마다 집계를 새로 읽어 화면 값이 따라간다", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    render(<BoardDiffStatsProbe taskIds={["task-1"]} isEnabled />);

    await waitFor(() => {
      expect(screen.getByTestId("additions").textContent).toBe("340");
    });

    mocks.getTaskDiffStats.mockResolvedValue({
      "task-1": { fileCount: 3, additions: 348, deletions: 76 },
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000);
    });

    expect(mocks.getTaskDiffStats).toHaveBeenCalledTimes(2);
    await waitFor(() => {
      expect(screen.getByTestId("additions").textContent).toBe("348");
    });
  });

  it("보드를 보고 있지 않으면 조회하지 않는다", () => {
    render(<BoardDiffStatsProbe taskIds={["task-1"]} isEnabled={false} />);

    expect(mocks.getTaskDiffStats).not.toHaveBeenCalled();
  });

  it("조회할 태스크가 없으면 조회하지 않는다", () => {
    render(<BoardDiffStatsProbe taskIds={[]} isEnabled />);

    expect(mocks.getTaskDiffStats).not.toHaveBeenCalled();
  });

  it("호출자가 매 렌더 새 배열을 넘겨도 같은 목록이면 다시 조회하지 않는다", async () => {
    const { rerender } = render(<BoardDiffStatsProbe taskIds={["task-1"]} isEnabled />);

    await waitFor(() => {
      expect(mocks.getTaskDiffStats).toHaveBeenCalledTimes(1);
    });
    rerender(<BoardDiffStatsProbe taskIds={["task-1"]} isEnabled />);

    expect(mocks.getTaskDiffStats).toHaveBeenCalledTimes(1);
  });
});
