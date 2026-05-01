import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import TaskQuickSearchDialog from "@/desktop/renderer/components/TaskQuickSearchDialog";

const mocks = vi.hoisted(() => ({
  getSearchableTasks: vi.fn(),
  push: vi.fn(),
}));

vi.mock("next-intl", () => ({
  useTranslations: (namespace: string) => (key: string) => `${namespace}.${key}`,
}));

vi.mock("@/desktop/renderer/actions/kanban", () => ({
  getSearchableTasks: (...args: unknown[]) => mocks.getSearchableTasks(...args),
}));

vi.mock("@/desktop/renderer/navigation", () => ({
  useRouter: () => ({
    push: (...args: unknown[]) => mocks.push(...args),
  }),
}));

describe("TaskQuickSearchDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getSearchableTasks.mockResolvedValue([
      {
        id: "task-local",
        title: "Local Task",
        branchName: "feat/local-search",
        projectId: "project-local",
        projectName: "kanvibe-web",
        sshHost: null,
        status: "todo",
        updatedAt: "2026-04-30T00:00:00.000Z",
      },
      {
        id: "task-remote",
        title: "Remote API Task",
        branchName: "feat/api-search",
        projectId: "project-remote",
        projectName: "api-server",
        sshHost: "devbox",
        status: "progress",
        updatedAt: "2026-04-30T01:00:00.000Z",
      },
      {
        id: "task-alert",
        title: "Alert automation",
        branchName: "feat/alert",
        projectId: "project-kanvibe",
        projectName: "kanvibe",
        sshHost: null,
        status: "todo",
        updatedAt: "2026-04-30T02:00:00.000Z",
      },
    ]);
  });

  it("단축키를 누르면 검색 다이얼로그를 열고 태스크 목록을 불러온다", async () => {
    render(<TaskQuickSearchDialog shortcut="Ctrl+K" />);

    fireEvent.keyDown(window, {
      key: "k",
      ctrlKey: true,
    });

    await waitFor(() => {
      expect(screen.getByRole("dialog")).toBeTruthy();
    });
    expect(mocks.getSearchableTasks).toHaveBeenCalledTimes(1);
    expect(screen.getByText("feat/local-search")).toBeTruthy();
    expect(screen.getByText("feat/api-search")).toBeTruthy();
  });

  it("검색 결과에서 원격 task에만 remote 배지와 호스트를 표시한다", async () => {
    render(<TaskQuickSearchDialog shortcut="Ctrl+K" />);

    fireEvent.keyDown(window, {
      key: "k",
      ctrlKey: true,
    });

    await waitFor(() => {
      expect(screen.getByRole("dialog")).toBeTruthy();
    });

    expect(screen.getByText("common.remote")).toBeTruthy();
    expect(screen.getByText("devbox")).toBeTruthy();
    expect(screen.queryByText("common.local")).toBeNull();
  });

  it("프로젝트명이나 브랜치명으로 검색한 뒤 Enter로 상세 페이지로 이동한다", async () => {
    render(<TaskQuickSearchDialog shortcut="Ctrl+K" />);

    fireEvent.keyDown(window, {
      key: "k",
      ctrlKey: true,
    });

    const input = await screen.findByRole("textbox");
    fireEvent.change(input, {
      target: { value: "api" },
    });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => {
      expect(mocks.push).toHaveBeenCalledWith("/task/task-remote");
    });
  });

  it("여러 토큰의 순서가 바뀌어도 같은 task를 검색할 수 있다", async () => {
    render(<TaskQuickSearchDialog shortcut="Ctrl+K" />);

    fireEvent.keyDown(window, {
      key: "k",
      ctrlKey: true,
    });

    const input = await screen.findByRole("textbox");
    fireEvent.change(input, {
      target: { value: "alert kanvibe feat" },
    });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => {
      expect(mocks.push).toHaveBeenCalledWith("/task/task-alert");
    });
  });

  it("Escape를 누르면 검색 다이얼로그를 닫는다", async () => {
    render(<TaskQuickSearchDialog shortcut="Ctrl+K" />);

    fireEvent.keyDown(window, {
      key: "k",
      ctrlKey: true,
    });

    const input = await screen.findByRole("textbox");
    fireEvent.keyDown(input, { key: "Escape" });

    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
  });
});
