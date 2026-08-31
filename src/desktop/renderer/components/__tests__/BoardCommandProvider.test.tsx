import { useEffect, type ReactNode } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BoardCommandProvider, useBoardCommands } from "@/desktop/renderer/components/BoardCommandProvider";

function CommandPaletteHarness() {
  const boardCommands = useBoardCommands();
  return (
    <span>{boardCommands.isCommandPaletteOpen ? "palette-open" : "palette-closed"}</span>
  );
}

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
  blockShortcuts = false,
}: {
  onToggleNotificationCenter: () => void;
  onOpenProjectFilter: () => void;
  onOpenCreateTaskModal: (defaults?: { projectId: string; baseBranch: string }) => void;
  blockShortcuts?: boolean;
}) {
  const boardCommands = useBoardCommands();

  useEffect(() => boardCommands.registerBoardHandlers({
    toggleNotificationCenter: onToggleNotificationCenter,
    openProjectFilter: onOpenProjectFilter,
    openCreateTaskModal: onOpenCreateTaskModal,
  }), [boardCommands, onOpenCreateTaskModal, onOpenProjectFilter, onToggleNotificationCenter]);

  useEffect(() => {
    if (!blockShortcuts) {
      return;
    }

    return boardCommands.registerShortcutBlocker();
  }, [blockShortcuts, boardCommands]);

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
      key: "f",
      ctrlKey: true,
      shiftKey: true,
    });

    expect(onToggleNotificationCenter).toHaveBeenCalledTimes(1);
    expect(onOpenProjectFilter).toHaveBeenCalledTimes(1);
    expect(screen.getByText("branch-enabled")).toBeTruthy();
  });

  it("opens the command palette from the global shortcut and blocks other board shortcuts while open", () => {
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
        <CommandPaletteHarness />
      </BoardCommandProvider>,
    );

    expect(screen.getByText("palette-closed")).toBeTruthy();

    fireEvent.keyDown(window, {
      key: "p",
      ctrlKey: true,
      shiftKey: true,
    });

    expect(screen.getByText("palette-open")).toBeTruthy();

    fireEvent.keyDown(window, {
      key: "i",
      ctrlKey: true,
      shiftKey: true,
    });

    expect(onToggleNotificationCenter).not.toHaveBeenCalled();
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

  it("ignores board shortcuts while a shortcut blocker is registered", () => {
    const onToggleNotificationCenter = vi.fn();
    const onOpenProjectFilter = vi.fn();
    const onOpenCreateTaskModal = vi.fn();

    renderWithRouter(
      <BoardCommandProvider>
        <BoardCommandHarness
          onToggleNotificationCenter={onToggleNotificationCenter}
          onOpenProjectFilter={onOpenProjectFilter}
          onOpenCreateTaskModal={onOpenCreateTaskModal}
          blockShortcuts
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
    fireEvent.keyDown(window, {
      key: "n",
      ctrlKey: true,
    });

    expect(onToggleNotificationCenter).not.toHaveBeenCalled();
    expect(onOpenProjectFilter).not.toHaveBeenCalled();
    expect(onOpenCreateTaskModal).not.toHaveBeenCalled();
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

  it("ignores desktop shortcut bridge events while a shortcut blocker is registered", () => {
    const onToggleNotificationCenter = vi.fn();
    const onOpenProjectFilter = vi.fn();
    const onOpenCreateTaskModal = vi.fn();
    let createTaskShortcutListener: (() => void) | null = null;
    let notificationShortcutListener: (() => void) | null = null;
    window.kanvibeDesktop = {
      isDesktop: true,
      onCreateTaskShortcut: vi.fn((listener: () => void) => {
        createTaskShortcutListener = listener;
        return vi.fn();
      }),
      onNotificationShortcut: vi.fn((listener: () => void) => {
        notificationShortcutListener = listener;
        return vi.fn();
      }),
    } as never;

    renderWithRouter(
      <BoardCommandProvider>
        <BoardCommandHarness
          onToggleNotificationCenter={onToggleNotificationCenter}
          onOpenProjectFilter={onOpenProjectFilter}
          onOpenCreateTaskModal={onOpenCreateTaskModal}
          blockShortcuts
        />
      </BoardCommandProvider>,
    );

    act(() => {
      createTaskShortcutListener?.();
      notificationShortcutListener?.();
    });

    expect(onOpenCreateTaskModal).not.toHaveBeenCalled();
    expect(onToggleNotificationCenter).not.toHaveBeenCalled();
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

  it("forwards the desktop notification shortcut event to a notification-only handler", () => {
    const onToggleNotificationCenter = vi.fn();
    let notificationShortcutListener: (() => void) | null = null;
    const unsubscribe = vi.fn();
    window.kanvibeDesktop = {
      isDesktop: true,
      onNotificationShortcut: vi.fn((listener: () => void) => {
        notificationShortcutListener = listener;
        return unsubscribe;
      }),
    } as never;

    const { unmount } = renderWithRouter(
      <BoardCommandProvider>
        <NotificationOnlyHarness onToggleNotificationCenter={onToggleNotificationCenter} />
      </BoardCommandProvider>,
    );

    act(() => {
      notificationShortcutListener?.();
    });

    expect(window.kanvibeDesktop.onNotificationShortcut).toHaveBeenCalledTimes(1);
    expect(onToggleNotificationCenter).toHaveBeenCalledTimes(1);

    unmount();
    expect(unsubscribe).toHaveBeenCalledTimes(1);
  });

  it("blocks Cmd/Ctrl+R so native reload shortcuts cannot run", () => {
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
    expect(mocks.triggerDesktopRefresh).not.toHaveBeenCalled();
  });
});
