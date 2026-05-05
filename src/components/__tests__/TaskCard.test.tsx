import { describe, it, expect, vi, beforeEach } from "vitest";
import { createEvent, fireEvent, render, screen } from "@testing-library/react";
import TaskCard from "../TaskCard";
import { TaskPriority } from "@/entities/TaskPriority";
import { TaskStatus } from "@/entities/KanbanTask";
import type { KanbanTask } from "@/entities/KanbanTask";

vi.mock("@hello-pangea/dnd", () => ({
  Draggable: ({ children }: { children: (provided: unknown, snapshot: unknown) => React.ReactNode }) =>
    children(
      {
        innerRef: vi.fn(),
        draggableProps: {
          "data-rfd-draggable-id": "test",
          style: { transform: "translate(12px, 18px)", transition: "transform 200ms ease" },
        },
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

  beforeEach(() => {
    vi.clearAllMocks();
  });

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

    const { container } = render(
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

    const card = screen.getByRole("link").firstElementChild as HTMLElement;
    expect(card.style.borderColor).toBe("rgb(101, 208, 138)");
    expect(container.querySelector(".bg-border-strong")).toBeNull();
  });

  it("should preserve draggable style while applying the project border color", () => {
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

    const card = screen.getByRole("link").firstElementChild as HTMLElement;
    expect(card.style.transform).toBe("translate(12px, 18px)");
    expect(card.style.transition).toBe("transform 200ms ease");
    expect(card.style.borderColor).toBe("rgb(101, 208, 138)");
  });

  it("should open the task context menu with Shift+Enter without following the link", () => {
    const task = createTask();
    const rectSpy = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
      x: 40,
      y: 80,
      left: 40,
      top: 80,
      right: 240,
      bottom: 120,
      width: 200,
      height: 40,
      toJSON: () => ({}),
    });

    render(<TaskCard task={task} index={0} onContextMenu={onContextMenu} />);

    const link = screen.getByRole("link");
    const event = createEvent.keyDown(link, { key: "Enter", shiftKey: true });
    fireEvent(link, event);

    expect(event.defaultPrevented).toBe(true);
    expect(onContextMenu).toHaveBeenCalledWith(task, { x: 52, y: 92 });

    rectSpy.mockRestore();
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
