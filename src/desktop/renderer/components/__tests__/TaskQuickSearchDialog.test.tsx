import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import TaskQuickSearchDialog from "@/desktop/renderer/components/TaskQuickSearchDialog";

const mocks = vi.hoisted(() => ({
  getSearchableTasks: vi.fn(),
  push: vi.fn(),
  requestCreateBranchTodo: vi.fn(),
  scrollIntoView: vi.fn(),
  setTaskQuickSearchOpen: vi.fn(),
}));

vi.mock("next-intl", () => ({
  useTranslations: (namespace: string) => (key: string) => `${namespace}.${key}`,
}));

vi.mock("@/desktop/renderer/actions/kanban", () => ({
  getSearchableTasks: (...args: unknown[]) => mocks.getSearchableTasks(...args),
}));

vi.mock("@/desktop/renderer/navigation", () => ({
  localizeHref: (href: string, currentLocale = "ko") => (
    href.startsWith("/") ? `/${currentLocale}${href}` : href
  ),
  usePathname: () => "/en",
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
    Object.defineProperty(Element.prototype, "scrollIntoView", {
      configurable: true,
      value: mocks.scrollIntoView,
    });
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
      {
        id: "task-dev-kanvibe",
        title: "Dev branch task",
        branchName: "dev",
        projectId: "project-kanvibe",
        projectName: "kanvibe",
        sshHost: null,
        status: "progress",
        updatedAt: "2026-04-30T03:00:00.000Z",
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
    expect(await screen.findByText("feat/local-search")).toBeTruthy();
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

    expect(await screen.findByText("common.remote")).toBeTruthy();
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

  it("검색 결과에서 상세 페이지로 이동할 때 terminal focus 요청을 보낸다", async () => {
    const focusListener = vi.fn();
    window.addEventListener("kanvibe:request-terminal-focus", focusListener);

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
    await waitFor(() => {
      expect(focusListener).toHaveBeenCalled();
    });

    window.removeEventListener("kanvibe:request-terminal-focus", focusListener);
  });

  it("검색 결과에서 Shift+Enter로 상세 페이지를 새 창에서 연다", async () => {
    const openWindow = vi.spyOn(window, "open").mockImplementation(() => null);

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
      key: "Enter",
      shiftKey: true,
    });

    await waitFor(() => {
      expect(openWindow).toHaveBeenCalledWith("/#/en/task/task-remote", "_blank", "noopener,noreferrer");
    });
    expect(mocks.push).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();
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

  it.each(["dev kanvibe", "kanvibe dev", "kanvibedev"])("%s 검색어로 project와 branch를 함께 찾는다", async (query) => {
    render(<TaskQuickSearchDialog shortcut="Ctrl+K" />);

    fireEvent.keyDown(window, {
      key: "k",
      ctrlKey: true,
    });

    const input = await screen.findByRole("textbox");
    fireEvent.change(input, {
      target: { value: query },
    });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => {
      expect(mocks.push).toHaveBeenCalledWith("/task/task-dev-kanvibe");
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

  it("should scroll the keyboard-selected result into view", async () => {
    // Given
    render(<TaskQuickSearchDialog shortcut="Ctrl+K" />);

    fireEvent.keyDown(window, {
      key: "k",
      ctrlKey: true,
    });

    const input = await screen.findByRole("textbox");
    await screen.findByText("feat/api-search");
    mocks.scrollIntoView.mockClear();

    // When
    fireEvent.keyDown(input, { key: "ArrowDown" });

    // Then
    await waitFor(() => {
      expect(mocks.scrollIntoView).toHaveBeenCalledWith({ block: "nearest" });
    });
  });
});
