import { useEffect, type ReactNode } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BoardCommandProvider, useBoardCommands } from "@/desktop/renderer/components/BoardCommandProvider";

const mocks = vi.hoisted(() => ({
  triggerDesktopRefresh: vi.fn(),
}));

vi.mock("@/desktop/renderer/utils/refresh", () => ({
  triggerDesktopRefresh: (...args: unknown[]) => mocks.triggerDesktopRefresh(...args),
}));

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

function renderWithRouter(children: ReactNode) {
  return render(
    <MemoryRouter initialEntries={["/ko"]}>
      {children}
    </MemoryRouter>,
  );
}

describe("BoardCommandProvider", () => {
  afterEach(() => {
    vi.clearAllMocks();
    delete window.kanvibeDesktop;
  });

  it("dispatches board shortcuts to registered handlers", () => {
    const onToggleNotificationCenter = vi.fn();
    const onOpenProjectFilter = vi.fn();
    const onOpenCreateTaskModal = vi.fn();

    renderWithRouter(
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

    renderWithRouter(
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

    renderWithRouter(
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

    renderWithRouter(
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

  it("forwards the desktop create task shortcut event to the registered board handler", () => {
    const onToggleNotificationCenter = vi.fn();
    const onOpenProjectFilter = vi.fn();
    const onOpenCreateTaskModal = vi.fn();
    let createTaskShortcutListener: (() => void) | null = null;
    const unsubscribe = vi.fn();
    window.kanvibeDesktop = {
      isDesktop: true,
      onCreateTaskShortcut: vi.fn((listener: () => void) => {
        createTaskShortcutListener = listener;
        return unsubscribe;
      }),
    };

    const { unmount } = renderWithRouter(
      <BoardCommandProvider>
        <BoardCommandHarness
          onToggleNotificationCenter={onToggleNotificationCenter}
          onOpenProjectFilter={onOpenProjectFilter}
          onOpenCreateTaskModal={onOpenCreateTaskModal}
        />
      </BoardCommandProvider>,
    );

    act(() => {
      createTaskShortcutListener?.();
    });

    expect(onOpenCreateTaskModal).toHaveBeenCalledTimes(1);
    expect(onOpenCreateTaskModal).toHaveBeenCalledWith();

    unmount();
    expect(unsubscribe).toHaveBeenCalledTimes(1);
  });

  it("dispatches the notification shortcut to a notification-only handler", () => {
    const onToggleNotificationCenter = vi.fn();

    renderWithRouter(
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

  it("refreshes all kanban data from the global refresh shortcut", () => {
    renderWithRouter(
      <BoardCommandProvider>
        <div />
      </BoardCommandProvider>,
    );

    const wasNotPrevented = fireEvent.keyDown(window, {
      key: "r",
      ctrlKey: true,
    });

    expect(wasNotPrevented).toBe(false);
    expect(mocks.triggerDesktopRefresh).toHaveBeenCalledWith("all");
  });
});
