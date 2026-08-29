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
  getCurrentShortcutPlatform,
  isBlockedShortcutEvent,
} from "@/desktop/renderer/utils/keyboardShortcut";
import { useShortcutBindings } from "@/desktop/renderer/utils/shortcutBindings";
import { findShortcutCommandForEvent } from "@/desktop/shared/shortcutBindings";

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
  registerShortcutBlocker: () => () => void;
  requestCreateBranchTodo: (defaults: BranchTodoDefaults) => void;
  setTaskQuickSearchOpen: (isOpen: boolean) => void;
}

const noopDisposer = () => {};
const defaultBoardCommandContextValue: BoardCommandContextValue = {
  canCreateBranchTodo: false,
  registerBoardHandlers: () => noopDisposer,
  registerNotificationCenterHandler: () => noopDisposer,
  registerShortcutBlocker: () => noopDisposer,
  requestCreateBranchTodo: () => {},
  setTaskQuickSearchOpen: () => {},
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
  const [canCreateBranchTodo, setCanCreateBranchTodo] = useState(false);
  const [isTaskQuickSearchOpen, setIsTaskQuickSearchOpen] = useState(false);
  const [shortcutBlockerCount, setShortcutBlockerCount] = useState(0);
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

      if (hasShortcutBlocker || isTaskQuickSearchOpen || shouldIgnoreGlobalShortcut(event.target)) {
        return;
      }

      const shortcutCommand = findShortcutCommandForEvent(shortcutBindings, event, shortcutPlatform);

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
  }, [hasShortcutBlocker, isTaskQuickSearchOpen, router, shortcutBindings, shortcutPlatform]);

  useEffect(() => {
    const unsubscribe = window.kanvibeDesktop?.onCreateTaskShortcut?.(() => {
      if (hasShortcutBlocker || isTaskQuickSearchOpen) {
        return;
      }

      handlersRef.current?.openCreateTaskModal();
    });

    return () => {
      unsubscribe?.();
    };
  }, [hasShortcutBlocker, isTaskQuickSearchOpen]);

  useEffect(() => {
    const unsubscribe = window.kanvibeDesktop?.onNotificationShortcut?.(() => {
      if (hasShortcutBlocker || isTaskQuickSearchOpen) {
        return;
      }

      notificationCenterHandlerRef.current?.();
    });

    return () => {
      unsubscribe?.();
    };
  }, [hasShortcutBlocker, isTaskQuickSearchOpen]);

  const value = useMemo<BoardCommandContextValue>(() => ({
    canCreateBranchTodo,
    registerBoardHandlers,
    registerNotificationCenterHandler,
    registerShortcutBlocker,
    requestCreateBranchTodo,
    setTaskQuickSearchOpen,
  }), [
    canCreateBranchTodo,
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
