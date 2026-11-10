import { lazy, Suspense, useEffect, useState } from "react";
import BoardRoute from "@/desktop/renderer/routes/BoardRoute";
import { getBoardViewMode, type BoardViewMode } from "@/desktop/renderer/actions/appSettings";
import { useRefreshSignal } from "@/desktop/renderer/utils/refresh";

const TaskWorkspaceRoute = lazy(() => import("@/desktop/renderer/routes/TaskWorkspaceRoute"));

/**
 * 같은 창에서 다시 보드로 돌아올 때 설정을 읽는 동안 화면이 비지 않도록 마지막 값을 기억한다.
 * 설정 자체는 AppSettings가 원본이고, 이 값은 렌더 직전의 표시용 사본이다.
 */
let lastKnownBoardViewMode: BoardViewMode | null = null;

/** 보드 경로에서 사용자가 마지막으로 고른 뷰(칸반 / 워크스페이스)를 보여 준다 */
export default function BoardViewRoute() {
  const settingsRefreshSignal = useRefreshSignal(["all", "settings"]);
  const [boardViewMode, setBoardViewModeState] = useState<BoardViewMode | null>(lastKnownBoardViewMode);

  useEffect(() => {
    let cancelled = false;

    getBoardViewMode()
      .catch((error) => {
        console.error("Failed to load board view mode:", error);
        return "kanban" as const;
      })
      .then((storedBoardViewMode) => {
        if (cancelled) {
          return;
        }

        lastKnownBoardViewMode = storedBoardViewMode;
        setBoardViewModeState(storedBoardViewMode);
      });

    return () => {
      cancelled = true;
    };
  }, [settingsRefreshSignal]);

  if (boardViewMode === null) {
    return <div className="min-h-screen bg-bg-page" />;
  }

  if (boardViewMode === "workspace") {
    return (
      <Suspense fallback={<div className="min-h-screen bg-bg-page" />}>
        <TaskWorkspaceRoute />
      </Suspense>
    );
  }

  return <BoardRoute />;
}
