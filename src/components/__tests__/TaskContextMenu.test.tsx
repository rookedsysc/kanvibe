import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import TaskContextMenu from "../TaskContextMenu";

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
    onDelete: vi.fn(),
    hasBranch: false,
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
    const deleteItem = screen.getByRole("menuitem", { name: "delete" });

    expect(document.activeElement).toBe(branchItem);

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
    const deleteItem = screen.getByRole("menuitem", { name: "delete" });

    expect(document.activeElement).toBe(createBranchTodoItem);

    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(document.activeElement).toBe(deleteItem);

    fireEvent.keyDown(deleteItem, { key: "Enter" });
    expect(props.onDelete).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(deleteItem, { key: "Escape" });
    expect(props.onClose).toHaveBeenCalledTimes(1);
  });
});
