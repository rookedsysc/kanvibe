import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { TaskDiffStatsBadge, TaskDiffSummary } from "@/components/TaskDiffStats";
import type { DiffFile } from "@/desktop/renderer/actions/diff";

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, unknown>) => (
    values ? `${key}:${JSON.stringify(values)}` : key
  ),
}));

vi.mock("@/desktop/renderer/navigation", () => ({
  Link: ({ children, href, title }: { children: React.ReactNode; href: string; title?: string }) => (
    <a href={href} title={title}>{children}</a>
  ),
}));

const changedFiles: DiffFile[] = [
  { path: "src/components/TaskCard.tsx", status: "modified", additions: 12, deletions: 3 },
  { path: "src/hooks/useTaskDiffStats.ts", status: "added", additions: 88, deletions: 0 },
];

describe("TaskDiffStatsBadge", () => {
  it("파일 수와 추가·삭제 줄 수를 보여준다", () => {
    render(<TaskDiffStatsBadge stats={{ fileCount: 12, additions: 348, deletions: 76 }} />);

    expect(screen.getByTestId("task-diff-file-count").textContent).toBe("12");
    expect(screen.getByTestId("task-diff-additions").textContent).toBe("348");
    expect(screen.getByTestId("task-diff-deletions").textContent).toBe("76");
  });

  it("변경이 없는 태스크에는 배지를 그리지 않는다", () => {
    render(<TaskDiffStatsBadge stats={{ fileCount: 0, additions: 0, deletions: 0 }} />);

    expect(screen.queryByTestId("task-card-diff-stats")).toBeNull();
  });

  it("아직 집계를 받지 못한 태스크에도 배지를 그리지 않는다", () => {
    render(<TaskDiffStatsBadge stats={undefined} />);

    expect(screen.queryByTestId("task-card-diff-stats")).toBeNull();
  });
});

describe("TaskDiffSummary", () => {
  it("파일 목록을 접은 채로 집계와 비율 막대를 보여준다", () => {
    render(<TaskDiffSummary taskId="task-1" files={changedFiles} />);

    expect(screen.getByTestId("task-diff-file-count").textContent).toBe("2");
    expect(screen.getByTestId("task-diff-additions").textContent).toBe("100");
    expect(screen.getByTestId("task-diff-deletions").textContent).toBe("3");
    expect(screen.getByTestId("task-diff-ratio-bar")).toBeTruthy();
    expect(screen.queryByTestId("task-diff-file-list")).toBeNull();
  });

  it("집계를 누르면 변경된 파일 목록을 펼친다", () => {
    render(<TaskDiffSummary taskId="task-1" files={changedFiles} />);

    fireEvent.click(screen.getByTestId("task-diff-summary-toggle"));

    const fileList = screen.getByTestId("task-diff-file-list");
    expect(fileList.textContent).toContain("src/components/TaskCard.tsx");
    expect(fileList.textContent).toContain("src/hooks/useTaskDiffStats.ts");
    expect(screen.getByTestId("task-diff-summary-toggle").getAttribute("aria-expanded")).toBe("true");
  });

  it("변경이 없으면 집계 없이 전체 diff 링크만 남긴다", () => {
    render(<TaskDiffSummary taskId="task-1" files={[]} />);

    expect(screen.queryByTestId("task-diff-summary-toggle")).toBeNull();
    expect(screen.queryByTestId("task-diff-ratio-bar")).toBeNull();
    expect(screen.getByRole("link").getAttribute("href")).toBe("/task/task-1/diff");
  });
});
