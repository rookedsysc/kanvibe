import { lazy, Suspense, useEffect, useMemo, useRef, type ReactNode } from "react";
import { IntlProvider } from "next-intl";
import { HashRouter, Navigate, Outlet, Route, Routes, useParams } from "react-router-dom";
import { BoardCommandProvider } from "@/desktop/renderer/components/BoardCommandProvider";
import BoardEventAlert from "@/desktop/renderer/components/BoardEventAlert";
import BackgroundSyncReviewDialog from "@/desktop/renderer/components/BackgroundSyncReviewDialog";
import NotificationListener from "@/desktop/renderer/components/NotificationListener";
import ReleaseUpdateDialog from "@/desktop/renderer/components/ReleaseUpdateDialog";
import TaskQuickSearchDialog from "@/desktop/renderer/components/TaskQuickSearchDialog";
import { DEFAULT_LOCALE, getSafeLocale, isSupportedLocale, messagesByLocale } from "@/desktop/renderer/utils/locales";
import { triggerDesktopRefresh } from "@/desktop/renderer/utils/refresh";
import BoardRoute from "@/desktop/renderer/routes/BoardRoute";
import { getThemePreference, type ThemePreference } from "@/desktop/renderer/actions/appSettings";
import { applyThemePreference, THEME_PREFERENCE_CHANGED_EVENT } from "@/desktop/renderer/utils/theme";
import {
  getCurrentShortcutPlatform,
  isTaskDetailRouteUrl,
} from "@/desktop/renderer/utils/keyboardShortcut";
import {
  isShortcutCaptureActive,
  isShortcutCaptureTarget,
  loadShortcutBindings,
  readShortcutBindings,
} from "@/desktop/renderer/utils/shortcutBindings";
import { findShortcutCommandForEvent, resolveTerminalTabCommand } from "@/desktop/shared/shortcutBindings";
import type { BoardEventPayload } from "@/lib/boardNotifier";
import type { TerminalTabShortcutCommand } from "@/desktop/shared/terminalTabs";

const BOARD_REFRESH_DEBOUNCE_MS = 250;

const DiffRoute = lazy(() => import("@/desktop/renderer/routes/DiffRoute"));
const NotFoundRoute = lazy(() => import("@/desktop/renderer/routes/NotFoundRoute"));
const AiAccountsRoute = lazy(() => import("@/desktop/renderer/routes/AiAccountsRoute"));
const PaneLayoutRoute = lazy(() => import("@/desktop/renderer/routes/PaneLayoutRoute"));
const SettingsRoute = lazy(() => import("@/desktop/renderer/routes/SettingsRoute"));
const ShortcutSettingsRoute = lazy(() => import("@/desktop/renderer/routes/ShortcutSettingsRoute"));
const TaskDetailRoute = lazy(() => import("@/desktop/renderer/routes/TaskDetailRoute"));

function RouteLoadingFallback() {
  return <div className="min-h-screen flex items-center justify-center bg-bg-page text-text-muted">Loading...</div>;
}

function DeferredRoute({ children }: { children: ReactNode }) {
  return (
    <Suspense fallback={<RouteLoadingFallback />}>
      {children}
    </Suspense>
  );
}

function ThemeController() {
  useEffect(() => {
    let cancelled = false;
    let currentThemePreference: ThemePreference = "system";
    let hasRuntimeThemePreference = false;

    const applyCurrentThemePreference = () => {
      applyThemePreference(currentThemePreference);
    };

    void getThemePreference().then((themePreference) => {
      if (cancelled || hasRuntimeThemePreference) {
        return;
      }

      currentThemePreference = themePreference;
      applyCurrentThemePreference();
    });

    const mediaQuery = window.matchMedia?.("(prefers-color-scheme: light)");
    mediaQuery?.addEventListener?.("change", applyCurrentThemePreference);

    const handleThemePreferenceChanged = (event: Event) => {
      hasRuntimeThemePreference = true;
      currentThemePreference = (event as CustomEvent<ThemePreference>).detail;
      applyCurrentThemePreference();
    };
    window.addEventListener(THEME_PREFERENCE_CHANGED_EVENT, handleThemePreferenceChanged);

    return () => {
      cancelled = true;
      mediaQuery?.removeEventListener?.("change", applyCurrentThemePreference);
      window.removeEventListener(THEME_PREFERENCE_CHANGED_EVENT, handleThemePreferenceChanged);
    };
  }, []);

  return null;
}

function LocaleShell() {
  const { locale } = useParams();
  const safeLocale = getSafeLocale(locale);
  const messages = useMemo(() => messagesByLocale[safeLocale], [safeLocale]);

  if (locale && !isSupportedLocale(locale)) {
    return <Navigate to={`/${DEFAULT_LOCALE}`} replace />;
  }

  return (
    <IntlProvider locale={safeLocale} messages={messages}>
      <ThemeController />
      <BoardCommandProvider>
        <TaskQuickSearchDialog />
        <NotificationListener />
        <ReleaseUpdateDialog />
        <BoardEventAlert />
        <BackgroundSyncReviewDialog />
        <Outlet />
      </BoardCommandProvider>
    </IntlProvider>
  );
}

export default function App() {
  const boardRefreshTimerRef = useRef<number | null>(null);

  /** 다른 창이 단축키를 바꾸면 main이 알려 준다. 그러지 않으면 이 창은 재시작 전까지 옛 조합으로 동작한다 */
  useEffect(() => {
    void loadShortcutBindings();

    return window.kanvibeDesktop?.onShortcutBindingsChanged?.(() => {
      void loadShortcutBindings();
    });
  }, []);

  useEffect(() => {
    const scheduleBoardRefresh = () => {
      if (boardRefreshTimerRef.current !== null) {
        return;
      }

      boardRefreshTimerRef.current = window.setTimeout(() => {
        boardRefreshTimerRef.current = null;
        triggerDesktopRefresh("all");
      }, BOARD_REFRESH_DEBOUNCE_MS);
    };

    const unsubscribeBoardEvents = window.kanvibeDesktop?.onBoardEvent?.((event: BoardEventPayload) => {
      if (event.type === "board-updated") {
        scheduleBoardRefresh();
      }
    }) ?? (() => {});

    return () => {
      unsubscribeBoardEvents();
      if (boardRefreshTimerRef.current !== null) {
        window.clearTimeout(boardRefreshTimerRef.current);
        boardRefreshTimerRef.current = null;
      }
    };
  }, []);

  /**
   * 창 닫기는 터미널이 없는 화면에서도 동작해야 하므로 라우트가 아니라 여기서 받는다.
   * 나머지 탭 명령은 터미널을 가진 태스크 상세만 의미가 있어 그쪽에서 처리한다.
   * 다른 단축키와 같이 Electron `before-input-event`와 렌더러 keydown 두 경로를 모두 받는다.
   */
  useEffect(() => {
    const closeWindowIfRequested = (command: TerminalTabShortcutCommand) => {
      /** 단축키를 녹화 중이면 그 조합은 명령이 아니라 녹화할 값이다 */
      if (isShortcutCaptureActive()) {
        return;
      }

      if (command.type === "close-window") {
        window.kanvibeDesktop?.closeCurrentWindow?.();
      }
    };

    function handleWindowCloseShortcut(event: KeyboardEvent) {
      if (isShortcutCaptureActive() || isShortcutCaptureTarget(event.target)) {
        return;
      }

      const command = resolveTerminalTabCommand(
        findShortcutCommandForEvent(readShortcutBindings(), event, getCurrentShortcutPlatform()),
        isTaskDetailRouteUrl(window.location.href),
      );
      if (command?.type !== "close-window") {
        return;
      }

      event.preventDefault();
      closeWindowIfRequested(command);
    }

    const unsubscribe = window.kanvibeDesktop?.onTerminalTabShortcut?.(closeWindowIfRequested) ?? (() => {});
    window.addEventListener("keydown", handleWindowCloseShortcut, { capture: true });

    return () => {
      unsubscribe();
      window.removeEventListener("keydown", handleWindowCloseShortcut, { capture: true });
    };
  }, []);

  return (
    <HashRouter>
      <Routes>
        <Route path="/" element={<Navigate to={`/${DEFAULT_LOCALE}`} replace />} />
        <Route path="/:locale" element={<LocaleShell />}>
          <Route index element={<BoardRoute />} />
          <Route path="ai-accounts" element={<DeferredRoute><AiAccountsRoute /></DeferredRoute>} />
          <Route path="pane-layout" element={<DeferredRoute><PaneLayoutRoute /></DeferredRoute>} />
          <Route path="settings" element={<DeferredRoute><SettingsRoute /></DeferredRoute>} />
          <Route path="settings/shortcuts" element={<DeferredRoute><ShortcutSettingsRoute /></DeferredRoute>} />
          <Route path="task/:id" element={<DeferredRoute><TaskDetailRoute /></DeferredRoute>} />
          <Route path="task/:id/diff" element={<DeferredRoute><DiffRoute /></DeferredRoute>} />
          <Route path="*" element={<DeferredRoute><NotFoundRoute /></DeferredRoute>} />
        </Route>
      </Routes>
    </HashRouter>
  );
}
