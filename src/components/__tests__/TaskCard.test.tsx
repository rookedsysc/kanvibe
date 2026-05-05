import { describe, it, expect, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import TaskCard from "../TaskCard";
import { TaskPriority } from "@/entities/TaskPriority";
import { TaskStatus } from "@/entities/KanbanTask";
import type { KanbanTask } from "@/entities/KanbanTask";

vi.mock("@hello-pangea/dnd", () => ({
  Draggable: ({ children }: { children: (provided: unknown, snapshot: unknown) => React.ReactNode }) =>
    children(
      {
        innerRef: vi.fn(),
        draggableProps: { "data-rfd-draggable-id": "test" },
        dragHandleProps: {},
      },
      { isDragging: false },
    ),
}));

vi.mock("@/desktop/renderer/navigation", () => ({
  Link: ({ children, ...props }: { children: React.ReactNode; href: string }) => (
    <a {...props}>{children}</a>
  ),
}));

vi.mock("next-intl", () => ({
  useLocale: () => "ko",
}));

function createTask(overrides: Partial<KanbanTask> = {}): KanbanTask {
  return {
    id: "task-1",
    title: "Test Task",
    description: null,
    status: TaskStatus.TODO,
    branchName: null,
    worktreePath: null,
    sessionType: null,
    sessionName: null,
    sshHost: null,
    agentType: null,
    project: null,
    projectId: null,
    baseBranch: null,
    prUrl: null,
    priority: null,
    displayOrder: 0,
    createdAt: new Date(),
    updatedAt: new Date(),
    ...overrides,
  };
}

describe("TaskCard - Priority Badge", () => {
  const onContextMenu = vi.fn();

  it("should not render priority badge when priority is null", () => {
    // Given
    const task = createTask({ priority: null });

    // When
    render(<TaskCard task={task} index={0} onContextMenu={onContextMenu} />);

    // Then
    expect(screen.queryByText("P3")).toBeNull();
    expect(screen.queryByText("P2")).toBeNull();
    expect(screen.queryByText("P1")).toBeNull();
  });

  it("should render P3 badge with low priority color when priority is LOW", () => {
    // Given
    const task = createTask({ priority: TaskPriority.LOW });

    // When
    render(<TaskCard task={task} index={0} onContextMenu={onContextMenu} />);

    // Then
    const badge = screen.getByText("P3");
    expect(badge).toBeTruthy();
    expect(badge.className).toContain("bg-priority-low-bg");
    expect(badge.className).toContain("text-priority-low-text");
  });

  it("should render P2 badge with medium priority color when priority is MEDIUM", () => {
    // Given
    const task = createTask({ priority: TaskPriority.MEDIUM });

    // When
    render(<TaskCard task={task} index={0} onContextMenu={onContextMenu} />);

    // Then
    const badge = screen.getByText("P2");
    expect(badge).toBeTruthy();
    expect(badge.className).toContain("bg-priority-medium-bg");
    expect(badge.className).toContain("text-priority-medium-text");
  });

  it("should render P1 badge with high priority color when priority is HIGH", () => {
    // Given
    const task = createTask({ priority: TaskPriority.HIGH });

    // When
    render(<TaskCard task={task} index={0} onContextMenu={onContextMenu} />);

    // Then
    const badge = screen.getByText("P1");
    expect(badge).toBeTruthy();
    expect(badge.className).toContain("bg-priority-high-bg");
    expect(badge.className).toContain("text-priority-high-text");
  });

  it("should render priority badge with ml-auto for right alignment", () => {
    // Given
    const task = createTask({ priority: TaskPriority.HIGH });

    // When
    render(<TaskCard task={task} index={0} onContextMenu={onContextMenu} />);

    // Then
    const badge = screen.getByText("P1");
    expect(badge.className).toContain("ml-auto");
  });

  it("should render project label above the task title with project color", () => {
    const task = createTask();

    render(
      <TaskCard
        task={task}
        index={0}
        onContextMenu={onContextMenu}
        projectName="kanvibe"
        projectColor="#65d08a"
      />,
    );

    const projectLabel = screen.getByText("kanvibe");
    expect(projectLabel).toBeTruthy();
    expect(projectLabel.style.color).toBe("rgb(101, 208, 138)");
  });

  it("should keep task detail navigation in the same window on desktop", async () => {
    const task = createTask();
    const openSpy = vi.spyOn(window, "open").mockImplementation(() => null);

    window.kanvibeDesktop = { isDesktop: true };

    render(<TaskCard task={task} index={0} onContextMenu={onContextMenu} />);

    const link = screen.getByRole("link");

    fireEvent.click(link);

    expect(link.getAttribute("href")).toBe("/task/task-1");
    expect(openSpy).not.toHaveBeenCalled();

    openSpy.mockRestore();
    delete window.kanvibeDesktop;
  });
});
