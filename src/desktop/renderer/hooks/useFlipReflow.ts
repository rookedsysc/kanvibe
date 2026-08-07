import { useLayoutEffect, useRef } from "react";

const TASK_CARD_SELECTOR = "[data-kanban-task-id]";
const REFLOW_DURATION_MS = 220;

/**
 * 목록 순서가 바뀔 때 카드가 이전 자리에서 새 자리로 미끄러져 오도록 보간한다.
 * 정렬 기준을 켜거나 드롭한 카드가 정렬된 자리로 되밀릴 때 카드가 순간이동하지 않게 한다.
 *
 * 드래그 라이브러리가 카드에 직접 transform을 걸어 두므로 그 값을 덮어쓰지 않도록
 * Web Animations API로 덧입힌다. animate를 지원하지 않는 환경에서는 전환 없이 바로 자리를 잡는다.
 */
export function useFlipReflow<T extends HTMLElement>(orderKey: string) {
  const containerRef = useRef<T>(null);
  const previousTopById = useRef(new Map<string, number>());

  useLayoutEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const nextTopById = new Map<string, number>();

    for (const card of container.querySelectorAll<HTMLElement>(TASK_CARD_SELECTOR)) {
      const taskId = card.dataset.kanbanTaskId;
      if (!taskId) continue;

      const nextTop = card.getBoundingClientRect().top;
      nextTopById.set(taskId, nextTop);

      const previousTop = previousTopById.current.get(taskId);
      if (previousTop === undefined || typeof card.animate !== "function") continue;

      const shift = previousTop - nextTop;
      if (Math.abs(shift) < 1) continue;

      card.animate(
        [{ transform: `translateY(${shift}px)` }, { transform: "translateY(0px)" }],
        { duration: REFLOW_DURATION_MS, easing: "ease-out", composite: "add" },
      );
    }

    previousTopById.current = nextTopById;
  }, [orderKey]);

  return containerRef;
}
