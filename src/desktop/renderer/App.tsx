import { useEffect, useMemo, useRef } from "react";
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
import DiffRoute from "@/desktop/renderer/routes/DiffRoute";
import NotFoundRoute from "@/desktop/renderer/routes/NotFoundRoute";
import PaneLayoutRoute from "@/desktop/renderer/routes/PaneLayoutRoute";
import TaskDetailRoute from "@/desktop/renderer/routes/TaskDetailRoute";
import type { BoardEventPayload } from "@/lib/boardNotifier";

const BOARD_REFRESH_DEBOUNCE_MS = 250;

function LocaleShell() {
  const { locale } = useParams();
  const safeLocale = getSafeLocale(locale);
  const messages = useMemo(() => messagesByLocale[safeLocale], [safeLocale]);

  if (locale && !isSupportedLocale(locale)) {
    return <Navigate to={`/${DEFAULT_LOCALE}`} replace />;
  }

  return (
    <IntlProvider locale={safeLocale} messages={messages}>
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
        triggerDesktopRefresh("board");
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
          <Route path="pane-layout" element={<PaneLayoutRoute />} />
          <Route path="task/:id" element={<TaskDetailRoute />} />
          <Route path="task/:id/diff" element={<DiffRoute />} />
          <Route path="*" element={<NotFoundRoute />} />
        </Route>
      </Routes>
    </HashRouter>
  );
}
