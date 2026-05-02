import type { ReactNode } from "react";
import { useEffect, useMemo, useState } from "react";
import { IntlProvider } from "next-intl";
import { HashRouter, Navigate, Outlet, Route, Routes, useParams } from "react-router-dom";
import LoginForm from "@/components/LoginForm";
import { BoardCommandProvider } from "@/desktop/renderer/components/BoardCommandProvider";
import BoardEventAlert from "@/desktop/renderer/components/BoardEventAlert";
import NotificationListener from "@/desktop/renderer/components/NotificationListener";
import TaskQuickSearchDialog from "@/desktop/renderer/components/TaskQuickSearchDialog";
import { getSessionState } from "@/desktop/renderer/actions/auth";
import { DEFAULT_LOCALE, getSafeLocale, isSupportedLocale, messagesByLocale } from "@/desktop/renderer/utils/locales";
import { INITIAL_DESKTOP_LOAD_TIMEOUT_MS } from "@/desktop/renderer/utils/loadingTimeout";
import { triggerDesktopRefresh } from "@/desktop/renderer/utils/refresh";
import BoardRoute from "@/desktop/renderer/routes/BoardRoute";
import DiffRoute from "@/desktop/renderer/routes/DiffRoute";
import NotFoundRoute from "@/desktop/renderer/routes/NotFoundRoute";
import PaneLayoutRoute from "@/desktop/renderer/routes/PaneLayoutRoute";
import TaskDetailRoute from "@/desktop/renderer/routes/TaskDetailRoute";
import type { BoardEventPayload } from "@/lib/boardNotifier";

interface SessionState {
  isAuthenticated: boolean;
}

function LocaleShell({ sessionLoading, isAuthenticated }: { sessionLoading: boolean; isAuthenticated: boolean }) {
  const { locale } = useParams();
  const safeLocale = getSafeLocale(locale);
  const messages = useMemo(() => messagesByLocale[safeLocale], [safeLocale]);

  if (locale && !isSupportedLocale(locale)) {
    return <Navigate to={`/${DEFAULT_LOCALE}`} replace />;
  }

  return (
    <IntlProvider locale={safeLocale} messages={messages}>
      <BoardCommandProvider>
        {!sessionLoading && (
          <>
            {isAuthenticated ? <TaskQuickSearchDialog /> : null}
            <NotificationListener />
            <BoardEventAlert />
          </>
        )}
        <Outlet />
      </BoardCommandProvider>
    </IntlProvider>
  );
}

function RouteLoadingFallback() {
  return <div className="min-h-screen flex items-center justify-center bg-bg-page text-text-muted">Loading...</div>;
}

function ProtectedRoute({ isAuthenticated, sessionLoading, children }: { isAuthenticated: boolean; sessionLoading: boolean; children: ReactNode }) {
  const { locale } = useParams();
  const safeLocale = getSafeLocale(locale);

  if (sessionLoading) {
    return <RouteLoadingFallback />;
  }

  if (!isAuthenticated) {
    return <Navigate to={`/${safeLocale}/login`} replace />;
  }

  return <>{children}</>;
}

function AnonymousRoute({ isAuthenticated, sessionLoading, children }: { isAuthenticated: boolean; sessionLoading: boolean; children: ReactNode }) {
  const { locale } = useParams();
  const safeLocale = getSafeLocale(locale);

  if (sessionLoading) {
    return <RouteLoadingFallback />;
  }

  if (isAuthenticated) {
    return <Navigate to={`/${safeLocale}`} replace />;
  }

  return <>{children}</>;
}

export default function App() {
  const [sessionState, setSessionState] = useState<SessionState | null>(null);

  useEffect(() => {
    let cancelled = false;
    let sessionLoadTimeout: number | null = null;

    const clearSessionLoadTimeout = () => {
      if (sessionLoadTimeout === null) {
        return;
      }

      window.clearTimeout(sessionLoadTimeout);
      sessionLoadTimeout = null;
    };

    const reloadSession = () => {
      clearSessionLoadTimeout();
      sessionLoadTimeout = window.setTimeout(() => {
        sessionLoadTimeout = null;
        if (!cancelled) {
          setSessionState((currentState) => currentState ?? { isAuthenticated: false });
        }
      }, INITIAL_DESKTOP_LOAD_TIMEOUT_MS);

      Promise.resolve()
        .then(() => getSessionState())
        .then((nextState) => {
          clearSessionLoadTimeout();
          if (!cancelled) {
            setSessionState(nextState);
          }
        })
        .catch((error) => {
          clearSessionLoadTimeout();
          console.error("Failed to load desktop session state:", error);
          if (!cancelled) {
            setSessionState({ isAuthenticated: false });
          }
        });
    };

    reloadSession();

    const handleSessionChanged = () => reloadSession();
    window.addEventListener("kanvibe:session-changed", handleSessionChanged);

    const unsubscribeBoardEvents = window.kanvibeDesktop?.onBoardEvent?.((event: BoardEventPayload) => {
      if (event.type === "board-updated") {
        triggerDesktopRefresh("board");
      }
    }) ?? (() => {});

    return () => {
      cancelled = true;
      clearSessionLoadTimeout();
      window.removeEventListener("kanvibe:session-changed", handleSessionChanged);
      unsubscribeBoardEvents();
    };
  }, []);

  const isAuthenticated = sessionState?.isAuthenticated ?? false;
  const sessionLoading = sessionState === null;

  return (
    <HashRouter>
      <Routes>
        <Route path="/" element={<Navigate to={`/${DEFAULT_LOCALE}${isAuthenticated ? "" : "/login"}`} replace />} />
        <Route path="/:locale" element={<LocaleShell sessionLoading={sessionLoading} isAuthenticated={isAuthenticated} />}>
          <Route index element={<ProtectedRoute isAuthenticated={isAuthenticated} sessionLoading={sessionLoading}><BoardRoute /></ProtectedRoute>} />
          <Route path="login" element={<AnonymousRoute isAuthenticated={isAuthenticated} sessionLoading={sessionLoading}><LoginForm /></AnonymousRoute>} />
          <Route path="pane-layout" element={<ProtectedRoute isAuthenticated={isAuthenticated} sessionLoading={sessionLoading}><PaneLayoutRoute /></ProtectedRoute>} />
          <Route path="task/:id" element={<ProtectedRoute isAuthenticated={isAuthenticated} sessionLoading={sessionLoading}><TaskDetailRoute /></ProtectedRoute>} />
          <Route path="task/:id/diff" element={<ProtectedRoute isAuthenticated={isAuthenticated} sessionLoading={sessionLoading}><DiffRoute /></ProtectedRoute>} />
          <Route path="*" element={<NotFoundRoute />} />
        </Route>
      </Routes>
    </HashRouter>
  );
}
