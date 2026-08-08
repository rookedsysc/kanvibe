import { useCallback, useEffect, useState } from "react";
import { getBoardSortPreference, setBoardSortPreference } from "@/desktop/renderer/actions/appSettings";
import { DEFAULT_BOARD_SORT_PREFERENCE, type BoardSortPreference } from "@/desktop/shared/boardSort";

/**
 * 보드 정렬 설정을 app_settings에서 읽고 쓴다.
 * 앱을 껐다 켜도 유지되어야 하므로 세션 저장소가 아니라 앱 설정 계층에 둔다.
 * 저장은 화면 반영과 분리해 낙관적으로 진행하고, 실패해도 이번 세션의 정렬은 그대로 유지한다.
 */
export function useBoardSortPreference() {
  const [preference, setPreference] = useState<BoardSortPreference>(DEFAULT_BOARD_SORT_PREFERENCE);

  useEffect(() => {
    let isSubscribed = true;

    getBoardSortPreference()
      .then((stored) => {
        if (isSubscribed) setPreference(stored);
      })
      .catch((error) => {
        console.error("보드 정렬 설정을 불러오지 못했습니다:", error);
      });

    return () => {
      isSubscribed = false;
    };
  }, []);

  const updatePreference = useCallback((next: BoardSortPreference) => {
    setPreference(next);
    void setBoardSortPreference(next).catch((error) => {
      console.error("보드 정렬 설정을 저장하지 못했습니다:", error);
    });
  }, []);

  return [preference, updatePreference] as const;
}
