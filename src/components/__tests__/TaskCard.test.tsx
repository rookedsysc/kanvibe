import { forwardRef } from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { createEvent, fireEvent, render, screen } from "@testing-library/react";
import TaskCard from "../TaskCard";
import { TaskPriority } from "@/entities/TaskPriority";
import { SessionType, TaskStatus } from "@/entities/KanbanTask";
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
  localizeHref: (href: string, currentLocale = "ko") => href.startsWith("/") ? `/${currentLocale}${href}` : href,
  Link: forwardRef<HTMLAnchorElement, React.AnchorHTMLAttributes<HTMLAnchorElement> & { href: string }>(
    function MockLink({ children, ...props }, ref) {
      return <a ref={ref} {...props}>{children}</a>;
    },
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

  it("should render session and remote badges with PR-aligned badge treatment", () => {
    const task = createTask({
      sessionType: SessionType.TMUX,
      sshHost: "devbox",
    });

    render(<TaskCard task={task} index={0} onContextMenu={onContextMenu} />);

    const sessionBadge = screen.getByText("tmux");
    const remoteBadge = screen.getByText("devbox");

    expect(sessionBadge.className).toContain("border-border-subtle");
    expect(sessionBadge.className).toContain("bg-tag-session-bg");
    expect(sessionBadge.className).toContain("text-tag-session-text");
    expect(sessionBadge.className).not.toContain("font-semibold");
    expect(sessionBadge.className).not.toContain("ring-1");
    expect(remoteBadge.className).toContain("border-border-subtle");
    expect(remoteBadge.className).toContain("bg-tag-ssh-bg");
    expect(remoteBadge.className).toContain("text-tag-ssh-text");
    expect(remoteBadge.className).not.toContain("font-semibold");
    expect(remoteBadge.className).not.toContain("ring-1");
  });

  it("should render base branch as a compact icon instead of a ribbon label", () => {
    const task = createTask();

    render(<TaskCard task={task} index={0} onContextMenu={onContextMenu} isBaseProject />);

    const baseIcon = screen.getByTestId("base-branch-icon");

    expect(baseIcon).toBeTruthy();
    expect(baseIcon.className).toContain("bg-tag-base-bg");
    expect(baseIcon.className).toContain("text-tag-base-text");
    expect(baseIcon.querySelector("svg")?.getAttribute("data-icon-name")).toBe("CrownIcon");
    expect(screen.queryByText("Base")).toBeNull();
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

    const card = screen.getByRole("link");
    expect(card.style.borderColor).toBe("rgb(101, 208, 138)");
    expect(container.querySelector(".bg-border-strong")).toBeNull();
  });

  it("should preserve draggable style on the focusable task link while applying the project border color", () => {
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

    const link = screen.getByRole("link");
    expect(link.getAttribute("data-rfd-draggable-id")).toBe("test");
    expect(link.style.transform).toBe("translate(12px, 18px)");
    expect(link.style.transition).toBe("transform 200ms ease");
    expect(link.style.borderColor).toBe("rgb(101, 208, 138)");
    expect(link.firstElementChild?.getAttribute("data-rfd-draggable-id")).toBeNull();
  });

  it("should move focus between tasks with ArrowUp and ArrowDown without scrolling", () => {
    const firstTask = createTask({ id: "task-1", title: "First task", status: TaskStatus.TODO });
    const secondTask = createTask({ id: "task-2", title: "Second task", status: TaskStatus.TODO });

    render(
      <>
        <TaskCard task={firstTask} index={0} onContextMenu={onContextMenu} />
        <TaskCard task={secondTask} index={1} onContextMenu={onContextMenu} />
      </>,
    );

    const firstLink = screen.getByRole("link", { name: /First task/ });
    const secondLink = screen.getByRole("link", { name: /Second task/ });

    firstLink.focus();
    const downEvent = createEvent.keyDown(firstLink, { key: "ArrowDown" });
    fireEvent(firstLink, downEvent);

    expect(downEvent.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(secondLink);

    const upEvent = createEvent.keyDown(secondLink, { key: "ArrowUp" });
    fireEvent(secondLink, upEvent);

    expect(upEvent.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(firstLink);
  });

  it("should move focus across kanban columns with ArrowLeft and ArrowRight", () => {
    const todoTask = createTask({ id: "task-1", title: "Todo task", status: TaskStatus.TODO });
    const progressTask = createTask({ id: "task-2", title: "Progress task", status: TaskStatus.PROGRESS });

    render(
      <>
        <TaskCard task={todoTask} index={0} onContextMenu={onContextMenu} />
        <TaskCard task={progressTask} index={0} onContextMenu={onContextMenu} />
      </>,
    );

    const todoLink = screen.getByRole("link", { name: /Todo task/ });
    const progressLink = screen.getByRole("link", { name: /Progress task/ });

    todoLink.focus();
    const rightEvent = createEvent.keyDown(todoLink, { key: "ArrowRight" });
    fireEvent(todoLink, rightEvent);

    expect(rightEvent.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(progressLink);

    const leftEvent = createEvent.keyDown(progressLink, { key: "ArrowLeft" });
    fireEvent(progressLink, leftEvent);

    expect(leftEvent.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(todoLink);
  });

  it("should open the task detail in a new window with Shift+Enter without following the link", () => {
    const task = createTask();
    const openWindow = vi.spyOn(window, "open").mockImplementation(() => null);

    render(<TaskCard task={task} index={0} onContextMenu={onContextMenu} />);

    const link = screen.getByRole("link");
    const event = createEvent.keyDown(link, { key: "Enter", shiftKey: true });
    fireEvent(link, event);

    expect(event.defaultPrevented).toBe(true);
    expect(openWindow).toHaveBeenCalledWith(`${window.location.origin}/#/ko/task/task-1`, "_blank", "noopener,noreferrer");
    expect(onContextMenu).not.toHaveBeenCalled();

    openWindow.mockRestore();
  });

  it("should open the task context menu with Shift+F10 without following the link", () => {
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
    const event = createEvent.keyDown(link, { key: "F10", shiftKey: true });
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
