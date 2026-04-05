import type { ReactNode } from "react";
import { useEffect, useMemo, useState } from "react";
import { IntlProvider } from "next-intl";
import { HashRouter, Navigate, Outlet, Route, Routes, useParams } from "react-router-dom";
import LoginForm from "@/components/LoginForm";
import NotificationListener from "@/desktop/renderer/components/NotificationListener";
import { getSessionState } from "@/desktop/renderer/actions/auth";
import { DEFAULT_LOCALE, getSafeLocale, isSupportedLocale, messagesByLocale } from "@/desktop/renderer/utils/locales";
import { triggerDesktopRefresh } from "@/desktop/renderer/utils/refresh";
import BoardRoute from "@/desktop/renderer/routes/BoardRoute";
import DiffRoute from "@/desktop/renderer/routes/DiffRoute";
import PaneLayoutRoute from "@/desktop/renderer/routes/PaneLayoutRoute";
import TaskDetailRoute from "@/desktop/renderer/routes/TaskDetailRoute";

interface SessionState {
  isAuthenticated: boolean;
}

function LocaleShell({ sessionLoading }: { sessionLoading: boolean }) {
  const { locale } = useParams();
  const safeLocale = getSafeLocale(locale);
  const messages = useMemo(() => messagesByLocale[safeLocale], [safeLocale]);

  if (locale && !isSupportedLocale(locale)) {
    return <Navigate to={`/${DEFAULT_LOCALE}`} replace />;
  }

  return (
    <IntlProvider locale={safeLocale} messages={messages}>
      {!sessionLoading && <NotificationListener />}
      <Outlet />
    </IntlProvider>
  );
}

function ProtectedRoute({ isAuthenticated, children }: { isAuthenticated: boolean; children: ReactNode }) {
  const { locale } = useParams();
  const safeLocale = getSafeLocale(locale);

  if (!isAuthenticated) {
    return <Navigate to={`/${safeLocale}/login`} replace />;
  }

  return <>{children}</>;
}

function AnonymousRoute({ isAuthenticated, children }: { isAuthenticated: boolean; children: ReactNode }) {
  const { locale } = useParams();
  const safeLocale = getSafeLocale(locale);

  if (isAuthenticated) {
    return <Navigate to={`/${safeLocale}`} replace />;
  }

  return <>{children}</>;
}

export default function App() {
  const [sessionState, setSessionState] = useState<SessionState | null>(null);

  useEffect(() => {
    let cancelled = false;

    const reloadSession = () => {
      getSessionState().then((nextState) => {
        if (!cancelled) {
          setSessionState(nextState);
        }
      });
    };

    reloadSession();

    const handleSessionChanged = () => reloadSession();
    window.addEventListener("kanvibe:session-changed", handleSessionChanged);

    const unsubscribeBoardEvents = window.kanvibeDesktop.onBoardEvent((event) => {
      if (event.type === "board-updated") {
        triggerDesktopRefresh();
      }
    });

    return () => {
      cancelled = true;
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
        <Route path="/:locale" element={<LocaleShell sessionLoading={sessionLoading} />}>
          <Route index element={sessionLoading ? <div className="min-h-screen flex items-center justify-center bg-bg-page text-text-muted">Loading...</div> : <ProtectedRoute isAuthenticated={isAuthenticated}><BoardRoute /></ProtectedRoute>} />
          <Route path="login" element={sessionLoading ? <div className="min-h-screen flex items-center justify-center bg-bg-page text-text-muted">Loading...</div> : <AnonymousRoute isAuthenticated={isAuthenticated}><LoginForm /></AnonymousRoute>} />
          <Route path="pane-layout" element={<ProtectedRoute isAuthenticated={isAuthenticated}><PaneLayoutRoute /></ProtectedRoute>} />
          <Route path="task/:id" element={<ProtectedRoute isAuthenticated={isAuthenticated}><TaskDetailRoute /></ProtectedRoute>} />
          <Route path="task/:id/diff" element={<ProtectedRoute isAuthenticated={isAuthenticated}><DiffRoute /></ProtectedRoute>} />
        </Route>
      </Routes>
    </HashRouter>
  );
}
