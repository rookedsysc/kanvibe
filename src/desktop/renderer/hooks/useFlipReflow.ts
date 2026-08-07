import { useLayoutEffect, useRef } from "react";

const TASK_CARD_SELECTOR = "[data-kanban-task-id]";
const REFLOW_DURATION_MS = 220;

/**
 * 목록 순서가 바뀔 때 카드가 이전 자리에서 새 자리로 미끄러져 오도록 보간한다.
 * 정렬 기준을 켜거나 드롭한 카드가 정렬된 자리로 되밀릴 때 카드가 순간이동하지 않게 한다.
 *
 * 드래그 라이브러리가 카드에 직접 transform을 걸어 두므로 그 값을 덮어쓰지 않도록
 * Web Animations API로 덧입힌다. animate를 지원하지 않는 환경에서는 전환 없이 바로 자리를 잡는다.
 *
 * 자리는 뷰포트가 아니라 컬럼 기준 offset으로 기억한다. 보드는 세로로 스크롤되므로 뷰포트 기준으로 재면
 * 스크롤한 뒤 정렬 기준을 켰을 때 스크롤한 거리만큼 어긋난 지점에서 카드가 날아온다.
 */
export function useFlipReflow<T extends HTMLElement>(orderKey: string) {
  const containerRef = useRef<T>(null);
  const previousOffsetById = useRef(new Map<string, number>());

  useLayoutEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const nextOffsetById = new Map<string, number>();
    const containerTop = container.getBoundingClientRect().top;

    for (const card of container.querySelectorAll<HTMLElement>(TASK_CARD_SELECTOR)) {
      const taskId = card.dataset.kanbanTaskId;
      if (!taskId) continue;

      const nextOffset = card.getBoundingClientRect().top - containerTop;
      nextOffsetById.set(taskId, nextOffset);

      const previousOffset = previousOffsetById.current.get(taskId);
      if (previousOffset === undefined || typeof card.animate !== "function") continue;

      const shift = previousOffset - nextOffset;
      if (Math.abs(shift) < 1) continue;

      card.animate(
        [{ transform: `translateY(${shift}px)` }, { transform: "translateY(0px)" }],
        { duration: REFLOW_DURATION_MS, easing: "ease-out", composite: "add" },
      );
    }

    previousOffsetById.current = nextOffsetById;
  }, [orderKey]);

  return containerRef;
}
