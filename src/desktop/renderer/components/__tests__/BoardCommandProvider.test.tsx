import { useEffect } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { BoardCommandProvider, useBoardCommands } from "@/desktop/renderer/components/BoardCommandProvider";

function BoardCommandHarness({
  onToggleNotificationCenter,
  onOpenProjectFilter,
  onOpenCreateTaskModal,
}: {
  onToggleNotificationCenter: () => void;
  onOpenProjectFilter: () => void;
  onOpenCreateTaskModal: (defaults?: { projectId: string; baseBranch: string }) => void;
}) {
  const boardCommands = useBoardCommands();

  useEffect(() => boardCommands.registerBoardHandlers({
    toggleNotificationCenter: onToggleNotificationCenter,
    openProjectFilter: onOpenProjectFilter,
    openCreateTaskModal: onOpenCreateTaskModal,
  }), [boardCommands, onOpenCreateTaskModal, onOpenProjectFilter, onToggleNotificationCenter]);

  return (
    <div>
      <button type="button" onClick={() => boardCommands.setTaskQuickSearchOpen(true)}>
        open quick search
      </button>
      <button type="button" onClick={() => boardCommands.setTaskQuickSearchOpen(false)}>
        close quick search
      </button>
      <button
        type="button"
        onClick={() => boardCommands.requestCreateBranchTodo({
          projectId: "project-1",
          baseBranch: "feat/from-search",
        })}
      >
        request branch todo
      </button>
      <span>{boardCommands.canCreateBranchTodo ? "branch-enabled" : "branch-disabled"}</span>
    </div>
  );
}

function NotificationOnlyHarness({ onToggleNotificationCenter }: { onToggleNotificationCenter: () => void }) {
  const boardCommands = useBoardCommands();

  useEffect(() => boardCommands.registerNotificationCenterHandler(onToggleNotificationCenter), [boardCommands, onToggleNotificationCenter]);

  return (
    <span>{boardCommands.canCreateBranchTodo ? "branch-enabled" : "branch-disabled"}</span>
  );
}

describe("BoardCommandProvider", () => {
  it("dispatches board shortcuts to registered handlers", () => {
    const onToggleNotificationCenter = vi.fn();
    const onOpenProjectFilter = vi.fn();
    const onOpenCreateTaskModal = vi.fn();

    render(
      <BoardCommandProvider>
        <BoardCommandHarness
          onToggleNotificationCenter={onToggleNotificationCenter}
          onOpenProjectFilter={onOpenProjectFilter}
          onOpenCreateTaskModal={onOpenCreateTaskModal}
        />
      </BoardCommandProvider>,
    );

    fireEvent.keyDown(window, {
      key: "i",
      ctrlKey: true,
      shiftKey: true,
    });
    fireEvent.keyDown(window, {
      key: "p",
      ctrlKey: true,
      shiftKey: true,
    });

    expect(onToggleNotificationCenter).toHaveBeenCalledTimes(1);
    expect(onOpenProjectFilter).toHaveBeenCalledTimes(1);
    expect(screen.getByText("branch-enabled")).toBeTruthy();
  });

  it("ignores board shortcuts while task quick search is open", () => {
    const onToggleNotificationCenter = vi.fn();
    const onOpenProjectFilter = vi.fn();
    const onOpenCreateTaskModal = vi.fn();

    render(
      <BoardCommandProvider>
        <BoardCommandHarness
          onToggleNotificationCenter={onToggleNotificationCenter}
          onOpenProjectFilter={onOpenProjectFilter}
          onOpenCreateTaskModal={onOpenCreateTaskModal}
        />
      </BoardCommandProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "open quick search" }));
    fireEvent.keyDown(window, {
      key: "i",
      ctrlKey: true,
      shiftKey: true,
    });
    fireEvent.keyDown(window, {
      key: "p",
      ctrlKey: true,
      shiftKey: true,
    });

    expect(onToggleNotificationCenter).not.toHaveBeenCalled();
    expect(onOpenProjectFilter).not.toHaveBeenCalled();
  });

  it("forwards branch todo requests to the registered board handler", () => {
    const onToggleNotificationCenter = vi.fn();
    const onOpenProjectFilter = vi.fn();
    const onOpenCreateTaskModal = vi.fn();

    render(
      <BoardCommandProvider>
        <BoardCommandHarness
          onToggleNotificationCenter={onToggleNotificationCenter}
          onOpenProjectFilter={onOpenProjectFilter}
          onOpenCreateTaskModal={onOpenCreateTaskModal}
        />
      </BoardCommandProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "request branch todo" }));

    expect(onOpenCreateTaskModal).toHaveBeenCalledWith({
      projectId: "project-1",
      baseBranch: "feat/from-search",
    });
  });

  it("opens the create task modal from the global new task shortcut", () => {
    const onToggleNotificationCenter = vi.fn();
    const onOpenProjectFilter = vi.fn();
    const onOpenCreateTaskModal = vi.fn();

    render(
      <BoardCommandProvider>
        <BoardCommandHarness
          onToggleNotificationCenter={onToggleNotificationCenter}
          onOpenProjectFilter={onOpenProjectFilter}
          onOpenCreateTaskModal={onOpenCreateTaskModal}
        />
      </BoardCommandProvider>,
    );

    fireEvent.keyDown(window, {
      key: "n",
      ctrlKey: true,
    });

    expect(onOpenCreateTaskModal).toHaveBeenCalledTimes(1);
    expect(onOpenCreateTaskModal).toHaveBeenCalledWith();
  });

  it("dispatches the notification shortcut to a notification-only handler", () => {
    const onToggleNotificationCenter = vi.fn();

    render(
      <BoardCommandProvider>
        <NotificationOnlyHarness onToggleNotificationCenter={onToggleNotificationCenter} />
      </BoardCommandProvider>,
    );

    fireEvent.keyDown(window, {
      key: "i",
      ctrlKey: true,
      shiftKey: true,
    });

    expect(onToggleNotificationCenter).toHaveBeenCalledTimes(1);
    expect(screen.getByText("branch-disabled")).toBeTruthy();
  });
});
