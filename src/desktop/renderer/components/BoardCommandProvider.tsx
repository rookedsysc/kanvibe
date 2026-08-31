"use client";

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type PropsWithChildren,
} from "react";
import { useRouter } from "@/desktop/renderer/navigation";
import { getFocusedBoardTaskCard } from "@/desktop/renderer/utils/focusedBoardTaskCard";
import {
  getCurrentShortcutPlatform,
  isBlockedShortcutEvent,
} from "@/desktop/renderer/utils/keyboardShortcut";
import { useShortcutBindings } from "@/desktop/renderer/utils/shortcutBindings";
import { findShortcutCommandForEvent } from "@/desktop/shared/shortcutBindings";
import type { TaskStatus } from "@/entities/KanbanTask";

export interface BranchTodoDefaults {
  projectId: string;
  baseBranch: string;
}

/** 커맨드 팔레트가 "지금 대상 task"로 삼을 task 상세 창의 등록 정보 */
export interface ActiveTaskContext {
  taskId: string;
  currentStatus: TaskStatus;
  moveTaskToStatus: (status: TaskStatus) => Promise<void> | void;
}

interface BoardCommandHandlers {
  toggleNotificationCenter: () => void;
  openProjectFilter: () => void;
  openCreateTaskModal: (defaults?: BranchTodoDefaults) => void;
  moveFocusedTaskToStatus?: (
    status: TaskStatus,
    taskIdOverride?: string | null,
  ) => "moved" | "missing-focus" | "missing-task";
}

interface BoardCommandContextValue {
  canCreateBranchTodo: boolean;
  registerBoardHandlers: (handlers: BoardCommandHandlers) => () => void;
  registerNotificationCenterHandler: (handler: () => void) => () => void;
  registerShortcutBlocker: () => () => void;
  requestCreateBranchTodo: (defaults: BranchTodoDefaults) => void;
  setTaskQuickSearchOpen: (isOpen: boolean) => void;
  isCommandPaletteOpen: boolean;
  openCommandPalette: () => void;
  closeCommandPalette: () => void;
  commandPaletteBoardFocusedTaskId: string | null;
  hasActiveTaskContext: boolean;
  activeTaskCurrentStatus: TaskStatus | null;
  registerActiveTaskContext: (context: ActiveTaskContext) => () => void;
  moveFocusedTaskToStatus: (status: TaskStatus) => "moved" | "missing-focus" | "missing-task";
  moveActiveTaskToStatus: (status: TaskStatus) => void;
}

const noopDisposer = () => {};
const defaultBoardCommandContextValue: BoardCommandContextValue = {
  canCreateBranchTodo: false,
  registerBoardHandlers: () => noopDisposer,
  registerNotificationCenterHandler: () => noopDisposer,
  registerShortcutBlocker: () => noopDisposer,
  requestCreateBranchTodo: () => {},
  setTaskQuickSearchOpen: () => {},
  isCommandPaletteOpen: false,
  openCommandPalette: () => {},
  closeCommandPalette: () => {},
  commandPaletteBoardFocusedTaskId: null,
  hasActiveTaskContext: false,
  activeTaskCurrentStatus: null,
  registerActiveTaskContext: () => noopDisposer,
  moveFocusedTaskToStatus: () => "missing-focus",
  moveActiveTaskToStatus: () => {},
};

const BoardCommandContext = createContext<BoardCommandContextValue | null>(null);
const BoardShortcutBlockerContext = createContext(false);

function shouldIgnoreGlobalShortcut(eventTarget: EventTarget | null) {
  if (!(eventTarget instanceof Element)) {
    return false;
  }

  if (eventTarget.closest('[data-shortcut-capture="true"]')) {
    return true;
  }

  if (
    eventTarget instanceof HTMLInputElement
    || eventTarget instanceof HTMLTextAreaElement
    || eventTarget instanceof HTMLSelectElement
  ) {
    return true;
  }

  return eventTarget.closest('[contenteditable="true"]') !== null;
}

export function BoardCommandProvider({ children }: PropsWithChildren) {
  const router = useRouter();
  const handlersRef = useRef<BoardCommandHandlers | null>(null);
  const notificationCenterHandlerRef = useRef<(() => void) | null>(null);
  const shortcutBlockerTokensRef = useRef<Set<symbol>>(new Set());
  const activeTaskContextRef = useRef<ActiveTaskContext | null>(null);
  const [canCreateBranchTodo, setCanCreateBranchTodo] = useState(false);
  const [isTaskQuickSearchOpen, setIsTaskQuickSearchOpen] = useState(false);
  const [shortcutBlockerCount, setShortcutBlockerCount] = useState(0);
  const [isCommandPaletteOpen, setIsCommandPaletteOpen] = useState(false);
  const [commandPaletteBoardFocusedTaskId, setCommandPaletteBoardFocusedTaskId] = useState<string | null>(null);
  const [hasActiveTaskContext, setHasActiveTaskContext] = useState(false);
  const [activeTaskCurrentStatus, setActiveTaskCurrentStatus] = useState<TaskStatus | null>(null);
  const shortcutPlatform = getCurrentShortcutPlatform();
  const shortcutBindings = useShortcutBindings();
  const hasShortcutBlocker = shortcutBlockerCount > 0;

  const registerBoardHandlers = useCallback((handlers: BoardCommandHandlers) => {
    handlersRef.current = handlers;
    notificationCenterHandlerRef.current = handlers.toggleNotificationCenter;
    setCanCreateBranchTodo(true);

    return () => {
      if (handlersRef.current === handlers) {
        handlersRef.current = null;
        setCanCreateBranchTodo(false);
      }

      if (notificationCenterHandlerRef.current === handlers.toggleNotificationCenter) {
        notificationCenterHandlerRef.current = null;
      }
    };
  }, []);

  const registerNotificationCenterHandler = useCallback((handler: () => void) => {
    notificationCenterHandlerRef.current = handler;

    return () => {
      if (notificationCenterHandlerRef.current === handler) {
        notificationCenterHandlerRef.current = null;
      }
    };
  }, []);

  const requestCreateBranchTodo = useCallback((defaults: BranchTodoDefaults) => {
    handlersRef.current?.openCreateTaskModal(defaults);
  }, []);

  const setTaskQuickSearchOpen = useCallback((isOpen: boolean) => {
    setIsTaskQuickSearchOpen(isOpen);
  }, []);

  const registerActiveTaskContext = useCallback((context: ActiveTaskContext) => {
    activeTaskContextRef.current = context;
    setHasActiveTaskContext(true);
    setActiveTaskCurrentStatus(context.currentStatus);

    return () => {
      if (activeTaskContextRef.current === context) {
        activeTaskContextRef.current = null;
        setHasActiveTaskContext(false);
        setActiveTaskCurrentStatus(null);
      }
    };
  }, []);

  const moveActiveTaskToStatus = useCallback((status: TaskStatus) => {
    void activeTaskContextRef.current?.moveTaskToStatus(status);
  }, []);

  const openCommandPalette = useCallback(() => {
    setCommandPaletteBoardFocusedTaskId(getFocusedBoardTaskCard()?.dataset.kanbanTaskId ?? null);
    setIsCommandPaletteOpen(true);
  }, []);

  const closeCommandPalette = useCallback(() => {
    setIsCommandPaletteOpen(false);
    setCommandPaletteBoardFocusedTaskId(null);
  }, []);

  const moveFocusedTaskToStatus = useCallback((status: TaskStatus) => (
    handlersRef.current?.moveFocusedTaskToStatus?.(status, commandPaletteBoardFocusedTaskId) ?? "missing-focus"
  ), [commandPaletteBoardFocusedTaskId]);

  const registerShortcutBlocker = useCallback(() => {
    const token = Symbol("shortcut-blocker");
    shortcutBlockerTokensRef.current.add(token);
    setShortcutBlockerCount(shortcutBlockerTokensRef.current.size);

    return () => {
      if (shortcutBlockerTokensRef.current.delete(token)) {
        setShortcutBlockerCount(shortcutBlockerTokensRef.current.size);
      }
    };
  }, []);

  useEffect(() => {
    function handleGlobalKeyDown(event: KeyboardEvent) {
      if (isBlockedShortcutEvent(event, shortcutPlatform)) {
        event.preventDefault();
        return;
      }

      if (
        hasShortcutBlocker
        || isTaskQuickSearchOpen
        || isCommandPaletteOpen
        || shouldIgnoreGlobalShortcut(event.target)
      ) {
        return;
      }

      const shortcutCommand = findShortcutCommandForEvent(shortcutBindings, event, shortcutPlatform);

      if (shortcutCommand === "commandPalette") {
        event.preventDefault();
        openCommandPalette();
        return;
      }

      if (shortcutCommand === "boardNotification") {
        if (!notificationCenterHandlerRef.current) {
          return;
        }

        event.preventDefault();
        notificationCenterHandlerRef.current();
        return;
      }

      if (shortcutCommand === "boardProjectFilter") {
        if (!handlersRef.current) {
          return;
        }

        event.preventDefault();
        handlersRef.current.openProjectFilter();
        return;
      }

      if (shortcutCommand === "createTask") {
        event.preventDefault();
        handlersRef.current?.openCreateTaskModal();
        return;
      }

      if (shortcutCommand === "pageBack") {
        event.preventDefault();
        router.back();
        return;
      }

      if (shortcutCommand === "pageForward") {
        event.preventDefault();
        router.forward();
      }
    }

    window.addEventListener("keydown", handleGlobalKeyDown);
    return () => {
      window.removeEventListener("keydown", handleGlobalKeyDown);
    };
  }, [hasShortcutBlocker, isCommandPaletteOpen, isTaskQuickSearchOpen, openCommandPalette, router, shortcutBindings, shortcutPlatform]);

  useEffect(() => {
    const unsubscribe = window.kanvibeDesktop?.onCreateTaskShortcut?.(() => {
      if (hasShortcutBlocker || isTaskQuickSearchOpen || isCommandPaletteOpen) {
        return;
      }

      handlersRef.current?.openCreateTaskModal();
    });

    return () => {
      unsubscribe?.();
    };
  }, [hasShortcutBlocker, isCommandPaletteOpen, isTaskQuickSearchOpen]);

  useEffect(() => {
    const unsubscribe = window.kanvibeDesktop?.onNotificationShortcut?.(() => {
      if (hasShortcutBlocker || isTaskQuickSearchOpen || isCommandPaletteOpen) {
        return;
      }

      notificationCenterHandlerRef.current?.();
    });

    return () => {
      unsubscribe?.();
    };
  }, [hasShortcutBlocker, isCommandPaletteOpen, isTaskQuickSearchOpen]);

  const value = useMemo<BoardCommandContextValue>(() => ({
    canCreateBranchTodo,
    registerBoardHandlers,
    registerNotificationCenterHandler,
    registerShortcutBlocker,
    requestCreateBranchTodo,
    setTaskQuickSearchOpen,
    isCommandPaletteOpen,
    openCommandPalette,
    closeCommandPalette,
    commandPaletteBoardFocusedTaskId,
    hasActiveTaskContext,
    activeTaskCurrentStatus,
    registerActiveTaskContext,
    moveFocusedTaskToStatus,
    moveActiveTaskToStatus,
  }), [
    activeTaskCurrentStatus,
    canCreateBranchTodo,
    closeCommandPalette,
    commandPaletteBoardFocusedTaskId,
    hasActiveTaskContext,
    isCommandPaletteOpen,
    moveActiveTaskToStatus,
    moveFocusedTaskToStatus,
    openCommandPalette,
    registerActiveTaskContext,
    registerBoardHandlers,
    registerNotificationCenterHandler,
    registerShortcutBlocker,
    requestCreateBranchTodo,
    setTaskQuickSearchOpen,
  ]);

  return (
    <BoardCommandContext.Provider value={value}>
      <BoardShortcutBlockerContext.Provider value={hasShortcutBlocker}>
        {children}
      </BoardShortcutBlockerContext.Provider>
    </BoardCommandContext.Provider>
  );
}

export function useBoardCommands() {
  return useContext(BoardCommandContext) ?? defaultBoardCommandContextValue;
}

export function useHasBoardShortcutBlocker() {
  return useContext(BoardShortcutBlockerContext);
}
