import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import TaskDetailTitleCard from "../TaskDetailTitleCard";
import { TaskStatus } from "@/entities/KanbanTask";
import type { KanbanTask } from "@/entities/KanbanTask";

const mockRefresh = vi.fn();
const mockUpdateTask = vi.fn().mockResolvedValue({});

vi.mock("@/i18n/navigation", () => ({
  useRouter: () => ({
    refresh: mockRefresh,
    push: vi.fn(),
    replace: vi.fn(),
    back: vi.fn(),
    forward: vi.fn(),
    prefetch: vi.fn(),
  }),
}));

vi.mock("next-intl", () => ({
  useTranslations: (namespace: string) => (key: string) => {
    const messages: Record<string, Record<string, string>> = {
      taskDetail: {
        addDescription: "설명 추가",
      },
      common: {
        cancel: "취소",
        save: "저장",
      },
    };
    return messages[namespace]?.[key] ?? key;
  },
}));

vi.mock("@/app/actions/kanban", () => ({
  updateTask: (...args: unknown[]) => mockUpdateTask(...args),
}));

vi.mock("@/components/TaskStatusBadge", () => ({
  default: ({ status }: { status: string }) => (
    <span data-testid="status-badge">{status}</span>
  ),
}));

vi.mock("@/components/ProjectBranchTasksModal", () => ({
  default: () => <div data-testid="branch-tasks-modal" />,
}));

function createTask(overrides: Partial<KanbanTask> = {}): KanbanTask {
  return {
    id: "task-1",
    title: "Test Task",
    description: null,
    status: TaskStatus.TODO,
    branchName: "feat/test-branch",
    worktreePath: null,
    sessionType: null,
    sessionName: null,
    sshHost: null,
    agentType: null,
    project: {
      id: "project-1",
      name: "kanvibe",
      repoPath: "/path/to/repo",
      defaultBranch: "main",
      sshHost: null,
      isWorktree: false,
      color: null,
      createdAt: new Date(),
    },
    projectId: "project-1",
    baseBranch: "main",
    prUrl: null,
    priority: null,
    displayOrder: 0,
    createdAt: new Date(),
    updatedAt: new Date(),
    ...overrides,
  };
}

describe("TaskDetailTitleCard - Description Editing", () => {
  beforeEach(() => {
    mockRefresh.mockClear();
    mockUpdateTask.mockClear();
  });

  it("should show 'addDescription' placeholder when description is null", () => {
    // Given
    const task = createTask({ description: null });

    // When
    render(<TaskDetailTitleCard task={task} taskId="task-1" />);

    // Then
    expect(screen.getByText("설명 추가")).toBeDefined();
  });

  it("should show description text when description exists", () => {
    // Given
    const task = createTask({ description: "기존 설명" });

    // When
    render(<TaskDetailTitleCard task={task} taskId="task-1" />);

    // Then
    expect(screen.getByText("기존 설명")).toBeDefined();
  });

  it("should enter edit mode when description area is clicked", async () => {
    // Given
    const task = createTask({ description: "기존 설명" });
    render(<TaskDetailTitleCard task={task} taskId="task-1" />);

    // When
    fireEvent.click(screen.getByText("기존 설명"));

    // Then
    const textarea = screen.getByRole("textbox");
    expect(textarea).toBeDefined();
    expect((textarea as HTMLTextAreaElement).value).toBe("기존 설명");
  });

  it("should enter edit mode with empty draft when placeholder is clicked", async () => {
    // Given
    const task = createTask({ description: null });
    render(<TaskDetailTitleCard task={task} taskId="task-1" />);

    // When
    fireEvent.click(screen.getByText("설명 추가"));

    // Then
    const textarea = screen.getByRole("textbox");
    expect(textarea).toBeDefined();
    expect((textarea as HTMLTextAreaElement).value).toBe("");
  });

  it("should show cancel and save buttons in edit mode", () => {
    // Given
    const task = createTask({ description: "기존 설명" });
    render(<TaskDetailTitleCard task={task} taskId="task-1" />);

    // When
    fireEvent.click(screen.getByText("기존 설명"));

    // Then
    expect(screen.getByText("취소")).toBeDefined();
    expect(screen.getByText("저장")).toBeDefined();
  });

  it("should exit edit mode and reset draft when cancel is clicked", async () => {
    // Given
    const user = userEvent.setup();
    const task = createTask({ description: "기존 설명" });
    render(<TaskDetailTitleCard task={task} taskId="task-1" />);
    fireEvent.click(screen.getByText("기존 설명"));

    // When - 텍스트 수정 후 취소
    const textarea = screen.getByRole("textbox");
    await user.clear(textarea);
    await user.type(textarea, "수정된 설명");
    fireEvent.click(screen.getByText("취소"));

    // Then - 편집 모드 종료, 원래 설명 표시
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(screen.getByText("기존 설명")).toBeDefined();
  });

  it("should exit edit mode when Escape key is pressed", () => {
    // Given
    const task = createTask({ description: "기존 설명" });
    render(<TaskDetailTitleCard task={task} taskId="task-1" />);
    fireEvent.click(screen.getByText("기존 설명"));

    // When
    const textarea = screen.getByRole("textbox");
    fireEvent.keyDown(textarea, { key: "Escape" });

    // Then
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(screen.getByText("기존 설명")).toBeDefined();
  });

  it("should call updateTask when save button is clicked with changed description", async () => {
    // Given
    const user = userEvent.setup();
    const task = createTask({ description: "기존 설명" });
    render(<TaskDetailTitleCard task={task} taskId="task-1" />);
    fireEvent.click(screen.getByText("기존 설명"));

    // When
    const textarea = screen.getByRole("textbox");
    await user.clear(textarea);
    await user.type(textarea, "새로운 설명");
    await act(async () => {
      fireEvent.click(screen.getByText("저장"));
    });

    // Then
    expect(mockUpdateTask).toHaveBeenCalledWith("task-1", { description: "새로운 설명" });
    expect(mockRefresh).toHaveBeenCalled();
  });

  it("should call updateTask via Cmd+Enter shortcut", async () => {
    // Given
    const user = userEvent.setup();
    const task = createTask({ description: "기존 설명" });
    render(<TaskDetailTitleCard task={task} taskId="task-1" />);
    fireEvent.click(screen.getByText("기존 설명"));

    // When
    const textarea = screen.getByRole("textbox");
    await user.clear(textarea);
    await user.type(textarea, "단축키 저장");
    await act(async () => {
      fireEvent.keyDown(textarea, { key: "Enter", metaKey: true });
    });

    // Then
    expect(mockUpdateTask).toHaveBeenCalledWith("task-1", { description: "단축키 저장" });
  });

  it("should call updateTask via Ctrl+Enter shortcut", async () => {
    // Given
    const user = userEvent.setup();
    const task = createTask({ description: "기존 설명" });
    render(<TaskDetailTitleCard task={task} taskId="task-1" />);
    fireEvent.click(screen.getByText("기존 설명"));

    // When
    const textarea = screen.getByRole("textbox");
    await user.clear(textarea);
    await user.type(textarea, "컨트롤 저장");
    await act(async () => {
      fireEvent.keyDown(textarea, { key: "Enter", ctrlKey: true });
    });

    // Then
    expect(mockUpdateTask).toHaveBeenCalledWith("task-1", { description: "컨트롤 저장" });
  });

  it("should not call updateTask when description is unchanged", async () => {
    // Given
    const task = createTask({ description: "기존 설명" });
    render(<TaskDetailTitleCard task={task} taskId="task-1" />);
    fireEvent.click(screen.getByText("기존 설명"));

    // When - 수정하지 않고 저장
    await act(async () => {
      fireEvent.click(screen.getByText("저장"));
    });

    // Then
    expect(mockUpdateTask).not.toHaveBeenCalled();
    expect(screen.queryByRole("textbox")).toBeNull();
  });

  it("should save null when draft is empty (whitespace only)", async () => {
    // Given
    const user = userEvent.setup();
    const task = createTask({ description: "기존 설명" });
    render(<TaskDetailTitleCard task={task} taskId="task-1" />);
    fireEvent.click(screen.getByText("기존 설명"));

    // When - 빈 값으로 저장
    const textarea = screen.getByRole("textbox");
    await user.clear(textarea);
    await user.type(textarea, "   ");
    await act(async () => {
      fireEvent.click(screen.getByText("저장"));
    });

    // Then - 빈 문자열이 아닌 null로 저장
    expect(mockUpdateTask).toHaveBeenCalledWith("task-1", { description: null });
  });

  it("should save new description when task originally had no description", async () => {
    // Given
    const user = userEvent.setup();
    const task = createTask({ description: null });
    render(<TaskDetailTitleCard task={task} taskId="task-1" />);
    fireEvent.click(screen.getByText("설명 추가"));

    // When
    const textarea = screen.getByRole("textbox");
    await user.type(textarea, "새 설명 추가");
    await act(async () => {
      fireEvent.click(screen.getByText("저장"));
    });

    // Then
    expect(mockUpdateTask).toHaveBeenCalledWith("task-1", { description: "새 설명 추가" });
    expect(mockRefresh).toHaveBeenCalled();
  });
});
