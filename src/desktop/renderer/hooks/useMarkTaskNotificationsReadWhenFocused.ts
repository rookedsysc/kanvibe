import { useEffect } from "react";
import { markTaskNotificationsRead } from "@/desktop/renderer/actions/notifications";
import { useIsWindowActive } from "@/desktop/renderer/hooks/useIsWindowActive";

/**
 * 상세 화면으로 포커스가 옮겨온 시점에만 그 task의 알림을 확인한 것으로 본다.
 * 창을 여러 개 띄워 두는 사용 방식이라, 뒤에 가려진 상세 창이 열려 있다는 이유만으로 읽음 처리하면
 * 사용자가 보지 못한 알림이 조용히 사라진다. 창이 활성으로 바뀌는 순간에만 읽음 처리하고,
 * 비활성인 동안 도착한 알림은 다시 포커스를 받을 때까지 안읽음으로 남긴다.
 */
export function useMarkTaskNotificationsReadWhenFocused(taskId: string | null) {
  const isWindowActive = useIsWindowActive();

  useEffect(() => {
    if (!taskId || !isWindowActive) {
      return;
    }

    void markTaskNotificationsRead(taskId).catch((error) => {
      console.error("알림 읽음 처리 실패:", error);
    });
  }, [taskId, isWindowActive]);
}
