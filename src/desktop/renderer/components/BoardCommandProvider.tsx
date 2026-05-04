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
import {
  SHORTCUTS,
  getCurrentShortcutPlatform,
  isBlockedShortcutEvent,
  matchShortcutEvent,
} from "@/desktop/renderer/utils/keyboardShortcut";

export const BOARD_NOTIFICATION_SHORTCUT = SHORTCUTS.boardNotification;
export const BOARD_PROJECT_FILTER_SHORTCUT = SHORTCUTS.boardProjectFilter;
export const CREATE_BRANCH_TODO_SHORTCUT = SHORTCUTS.createTask;
export const PAGE_BACK_SHORTCUT = SHORTCUTS.pageBack;
export const PAGE_FORWARD_SHORTCUT = SHORTCUTS.pageForward;

export interface BranchTodoDefaults {
  projectId: string;
  baseBranch: string;
}

interface BoardCommandHandlers {
  toggleNotificationCenter: () => void;
  openProjectFilter: () => void;
  openCreateTaskModal: (defaults?: BranchTodoDefaults) => void;
}

interface BoardCommandContextValue {
  canCreateBranchTodo: boolean;
  registerBoardHandlers: (handlers: BoardCommandHandlers) => () => void;
  registerNotificationCenterHandler: (handler: () => void) => () => void;
  requestCreateBranchTodo: (defaults: BranchTodoDefaults) => void;
  setTaskQuickSearchOpen: (isOpen: boolean) => void;
}

const noopDisposer = () => {};
const defaultBoardCommandContextValue: BoardCommandContextValue = {
  canCreateBranchTodo: false,
  registerBoardHandlers: () => noopDisposer,
  registerNotificationCenterHandler: () => noopDisposer,
  requestCreateBranchTodo: () => {},
  setTaskQuickSearchOpen: () => {},
};

const BoardCommandContext = createContext<BoardCommandContextValue | null>(null);

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
  const [canCreateBranchTodo, setCanCreateBranchTodo] = useState(false);
  const [isTaskQuickSearchOpen, setIsTaskQuickSearchOpen] = useState(false);
  const shortcutPlatform = getCurrentShortcutPlatform();

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

  useEffect(() => {
    function handleGlobalKeyDown(event: KeyboardEvent) {
      if (isBlockedShortcutEvent(event, shortcutPlatform)) {
        event.preventDefault();
        return;
      }

      if (isTaskQuickSearchOpen || shouldIgnoreGlobalShortcut(event.target)) {
        return;
      }

      if (matchShortcutEvent(event, BOARD_NOTIFICATION_SHORTCUT, shortcutPlatform)) {
        if (!notificationCenterHandlerRef.current) {
          return;
        }

        event.preventDefault();
        notificationCenterHandlerRef.current();
        return;
      }

      if (matchShortcutEvent(event, BOARD_PROJECT_FILTER_SHORTCUT, shortcutPlatform)) {
        if (!handlersRef.current) {
          return;
        }

        event.preventDefault();
        handlersRef.current.openProjectFilter();
        return;
      }

      if (matchShortcutEvent(event, CREATE_BRANCH_TODO_SHORTCUT, shortcutPlatform)) {
        event.preventDefault();
        handlersRef.current?.openCreateTaskModal();
        return;
      }

      if (matchShortcutEvent(event, PAGE_BACK_SHORTCUT, shortcutPlatform)) {
        event.preventDefault();
        router.back();
        return;
      }

      if (matchShortcutEvent(event, PAGE_FORWARD_SHORTCUT, shortcutPlatform)) {
        event.preventDefault();
        router.forward();
      }
    }

    window.addEventListener("keydown", handleGlobalKeyDown);
    return () => {
      window.removeEventListener("keydown", handleGlobalKeyDown);
    };
  }, [isTaskQuickSearchOpen, router, shortcutPlatform]);

  useEffect(() => {
    const unsubscribe = window.kanvibeDesktop?.onCreateTaskShortcut?.(() => {
      if (isTaskQuickSearchOpen) {
        return;
      }

      handlersRef.current?.openCreateTaskModal();
    });

    return () => {
      unsubscribe?.();
    };
  }, [isTaskQuickSearchOpen]);

  useEffect(() => {
    const unsubscribe = window.kanvibeDesktop?.onNotificationShortcut?.(() => {
      if (isTaskQuickSearchOpen) {
        return;
      }

      notificationCenterHandlerRef.current?.();
    });

    return () => {
      unsubscribe?.();
    };
  }, [isTaskQuickSearchOpen]);

  const value = useMemo<BoardCommandContextValue>(() => ({
    canCreateBranchTodo,
    registerBoardHandlers,
    registerNotificationCenterHandler,
    requestCreateBranchTodo,
    setTaskQuickSearchOpen,
  }), [canCreateBranchTodo, registerBoardHandlers, registerNotificationCenterHandler, requestCreateBranchTodo, setTaskQuickSearchOpen]);

  return (
    <BoardCommandContext.Provider value={value}>
      {children}
    </BoardCommandContext.Provider>
  );
}

export function useBoardCommands() {
  return useContext(BoardCommandContext) ?? defaultBoardCommandContextValue;
}
