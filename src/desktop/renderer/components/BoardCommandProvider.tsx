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
import { matchShortcutEvent } from "@/desktop/renderer/utils/keyboardShortcut";
import { triggerDesktopRefresh } from "@/desktop/renderer/utils/refresh";

export const BOARD_NOTIFICATION_SHORTCUT = "Mod+Shift+I";
export const BOARD_PROJECT_FILTER_SHORTCUT = "Mod+Shift+P";
export const BOARD_REFRESH_SHORTCUT = "Mod+R";
export const CREATE_BRANCH_TODO_SHORTCUT = "Mod+N";
export const PAGE_BACK_SHORTCUT = "Mod+[";
export const PAGE_FORWARD_SHORTCUT = "Mod+]";

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

function isMacLikePlatform() {
  return typeof navigator !== "undefined"
    && (navigator.userAgent.includes("Mac") || navigator.platform.toLowerCase().includes("mac"));
}

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
  const isMacLike = isMacLikePlatform();

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
      if (isTaskQuickSearchOpen || shouldIgnoreGlobalShortcut(event.target)) {
        return;
      }

      if (matchShortcutEvent(event, BOARD_REFRESH_SHORTCUT, isMacLike)) {
        event.preventDefault();
        triggerDesktopRefresh("all");
        return;
      }

      if (matchShortcutEvent(event, BOARD_NOTIFICATION_SHORTCUT, isMacLike)) {
        if (!notificationCenterHandlerRef.current) {
          return;
        }

        event.preventDefault();
        notificationCenterHandlerRef.current();
        return;
      }

      if (matchShortcutEvent(event, BOARD_PROJECT_FILTER_SHORTCUT, isMacLike)) {
        if (!handlersRef.current) {
          return;
        }

        event.preventDefault();
        handlersRef.current.openProjectFilter();
        return;
      }

      if (matchShortcutEvent(event, CREATE_BRANCH_TODO_SHORTCUT, isMacLike)) {
        event.preventDefault();
        handlersRef.current?.openCreateTaskModal();
        return;
      }

      if (matchShortcutEvent(event, PAGE_BACK_SHORTCUT, isMacLike)) {
        event.preventDefault();
        router.back();
        return;
      }

      if (matchShortcutEvent(event, PAGE_FORWARD_SHORTCUT, isMacLike)) {
        event.preventDefault();
        router.forward();
      }
    }

    window.addEventListener("keydown", handleGlobalKeyDown);
    return () => {
      window.removeEventListener("keydown", handleGlobalKeyDown);
    };
  }, [isMacLike, isTaskQuickSearchOpen, router]);

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
