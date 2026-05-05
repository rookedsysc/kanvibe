import { lazy, Suspense, useEffect, useMemo, useRef, type ReactNode } from "react";
import { IntlProvider } from "next-intl";
import { HashRouter, Navigate, Outlet, Route, Routes, useParams } from "react-router-dom";
import { BoardCommandProvider } from "@/desktop/renderer/components/BoardCommandProvider";
import BoardEventAlert from "@/desktop/renderer/components/BoardEventAlert";
import BackgroundSyncReviewDialog from "@/desktop/renderer/components/BackgroundSyncReviewDialog";
import NotificationListener from "@/desktop/renderer/components/NotificationListener";
import TaskQuickSearchDialog from "@/desktop/renderer/components/TaskQuickSearchDialog";
import { DEFAULT_LOCALE, getSafeLocale, isSupportedLocale, messagesByLocale } from "@/desktop/renderer/utils/locales";
import { triggerDesktopRefresh } from "@/desktop/renderer/utils/refresh";
import BoardRoute from "@/desktop/renderer/routes/BoardRoute";
import { getThemePreference, type ThemePreference } from "@/desktop/renderer/actions/appSettings";
import { applyThemePreference, THEME_PREFERENCE_CHANGED_EVENT } from "@/desktop/renderer/utils/theme";
import type { BoardEventPayload } from "@/lib/boardNotifier";

const BOARD_REFRESH_DEBOUNCE_MS = 250;

const DiffRoute = lazy(() => import("@/desktop/renderer/routes/DiffRoute"));
const NotFoundRoute = lazy(() => import("@/desktop/renderer/routes/NotFoundRoute"));
const PaneLayoutRoute = lazy(() => import("@/desktop/renderer/routes/PaneLayoutRoute"));
const SettingsRoute = lazy(() => import("@/desktop/renderer/routes/SettingsRoute"));
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
        <BoardEventAlert />
        <BackgroundSyncReviewDialog />
        <Outlet />
      </BoardCommandProvider>
    </IntlProvider>
  );
}

export default function App() {
  const boardRefreshTimerRef = useRef<number | null>(null);

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

  return (
    <HashRouter>
      <Routes>
        <Route path="/" element={<Navigate to={`/${DEFAULT_LOCALE}`} replace />} />
        <Route path="/:locale" element={<LocaleShell />}>
          <Route index element={<BoardRoute />} />
          <Route path="pane-layout" element={<DeferredRoute><PaneLayoutRoute /></DeferredRoute>} />
          <Route path="settings" element={<DeferredRoute><SettingsRoute /></DeferredRoute>} />
          <Route path="task/:id" element={<DeferredRoute><TaskDetailRoute /></DeferredRoute>} />
          <Route path="task/:id/diff" element={<DeferredRoute><DiffRoute /></DeferredRoute>} />
          <Route path="*" element={<DeferredRoute><NotFoundRoute /></DeferredRoute>} />
        </Route>
      </Routes>
    </HashRouter>
  );
}
