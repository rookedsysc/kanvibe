const TASK_CARD_SELECTOR = "[data-kanban-task-card='true']";

/** 현재 키보드 포커스가 놓인 보드 태스크 카드. 없으면 null */
export function getFocusedBoardTaskCard() {
  const activeElement = document.activeElement;
  return activeElement instanceof Element
    ? activeElement.closest<HTMLAnchorElement>(TASK_CARD_SELECTOR)
    : null;
}
