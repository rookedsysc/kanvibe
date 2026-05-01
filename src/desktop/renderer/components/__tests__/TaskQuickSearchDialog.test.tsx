import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import TaskQuickSearchDialog from "@/desktop/renderer/components/TaskQuickSearchDialog";

const mocks = vi.hoisted(() => ({
  getSearchableTasks: vi.fn(),
  push: vi.fn(),
  requestCreateBranchTodo: vi.fn(),
  setTaskQuickSearchOpen: vi.fn(),
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

vi.mock("@/desktop/renderer/components/BoardCommandProvider", () => ({
  CREATE_BRANCH_TODO_SHORTCUT: "Mod+N",
  useBoardCommands: () => ({
    requestCreateBranchTodo: mocks.requestCreateBranchTodo,
    setTaskQuickSearchOpen: mocks.setTaskQuickSearchOpen,
    canCreateBranchTodo: true,
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

  it("검색 결과에 원격과 로컬을 구분해서 표시한다", async () => {
    render(<TaskQuickSearchDialog shortcut="Ctrl+K" />);

    fireEvent.keyDown(window, {
      key: "k",
      ctrlKey: true,
    });

    await waitFor(() => {
      expect(screen.getByRole("dialog")).toBeTruthy();
    });

    expect(screen.getByText("common.local")).toBeTruthy();
    expect(screen.getByText("common.remote")).toBeTruthy();
    expect(screen.getByText("devbox")).toBeTruthy();
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

  it("선택된 검색 결과에서 Ctrl+N으로 branch TODO 생성을 요청한다", async () => {
    render(<TaskQuickSearchDialog shortcut="Ctrl+K" />);

    fireEvent.keyDown(window, {
      key: "k",
      ctrlKey: true,
    });

    const input = await screen.findByRole("textbox");
    fireEvent.change(input, {
      target: { value: "api" },
    });
    fireEvent.keyDown(input, {
      key: "n",
      ctrlKey: true,
    });

    await waitFor(() => {
      expect(mocks.requestCreateBranchTodo).toHaveBeenCalledWith({
        projectId: "project-remote",
        baseBranch: "feat/api-search",
      });
    });
    expect(screen.queryByRole("dialog")).toBeNull();
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
