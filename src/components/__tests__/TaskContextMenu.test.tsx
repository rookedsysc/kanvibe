import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import TaskContextMenu from "../TaskContextMenu";
import { TaskStatus } from "@/entities/KanbanTask";

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string) => key,
}));

function renderMenu(overrides: Partial<React.ComponentProps<typeof TaskContextMenu>> = {}) {
  const props: React.ComponentProps<typeof TaskContextMenu> = {
    x: 24,
    y: 36,
    onClose: vi.fn(),
    onBranch: vi.fn(),
    onCreateBranchTodo: vi.fn(),
    onStatusChange: vi.fn(),
    onDelete: vi.fn(),
    hasBranch: false,
    currentStatus: TaskStatus.TODO,
    statusOptions: [
      { status: TaskStatus.TODO, label: "Todo", colorClass: "bg-status-todo" },
      { status: TaskStatus.PROGRESS, label: "Progress", colorClass: "bg-status-progress" },
      { status: TaskStatus.PENDING, label: "Pending", colorClass: "bg-status-pending" },
      { status: TaskStatus.REVIEW, label: "Review", colorClass: "bg-status-review" },
      { status: TaskStatus.DONE, label: "Done", colorClass: "bg-status-done" },
    ],
    ...overrides,
  };

  render(<TaskContextMenu {...props} />);

  return props;
}

describe("TaskContextMenu keyboard interaction", () => {
  it("focuses the first menu item and cycles through items with arrow keys", () => {
    renderMenu();

    const menu = screen.getByRole("menu");
    const branchItem = screen.getByRole("menuitem", { name: "branchOff" });
    const statusItem = screen.getByRole("menuitem", { name: "changeStatus Todo" });
    const deleteItem = screen.getByRole("menuitem", { name: "delete" });

    expect(document.activeElement).toBe(branchItem);

    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(document.activeElement).toBe(statusItem);

    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(document.activeElement).toBe(deleteItem);

    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(document.activeElement).toBe(branchItem);

    fireEvent.keyDown(menu, { key: "ArrowUp" });
    expect(document.activeElement).toBe(deleteItem);
  });

  it("activates the focused item with Enter and closes with Escape", () => {
    const props = renderMenu({ hasBranch: true });

    const menu = screen.getByRole("menu");
    const createBranchTodoItem = screen.getByRole("menuitem", { name: "createBranchTodo" });
    const statusItem = screen.getByRole("menuitem", { name: "changeStatus Todo" });
    const deleteItem = screen.getByRole("menuitem", { name: "delete" });

    expect(document.activeElement).toBe(createBranchTodoItem);

    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(document.activeElement).toBe(statusItem);

    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(document.activeElement).toBe(deleteItem);

    fireEvent.keyDown(deleteItem, { key: "Enter" });
    expect(props.onDelete).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(deleteItem, { key: "Escape" });
    expect(props.onClose).toHaveBeenCalledTimes(1);
  });

  it("opens the status dropdown and changes status with keyboard only", () => {
    const props = renderMenu();

    const menu = screen.getByRole("menu");
    const statusItem = screen.getByRole("menuitem", { name: "changeStatus Todo" });

    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(document.activeElement).toBe(statusItem);

    fireEvent.keyDown(statusItem, { key: "Enter" });

    const statusOptions = screen.getAllByRole("menuitemradio");
    expect(statusOptions.map((option) => option.textContent)).toEqual([
      "Todo",
      "Progress",
      "Pending",
      "Review",
      "Done",
    ]);
    expect(document.activeElement).toBe(statusOptions[0]);

    fireEvent.keyDown(statusOptions[0], { key: "ArrowDown" });
    expect(document.activeElement).toBe(statusOptions[1]);

    fireEvent.keyDown(statusOptions[1], { key: "Enter" });
    expect(props.onStatusChange).toHaveBeenCalledWith(TaskStatus.PROGRESS);
  });
});
