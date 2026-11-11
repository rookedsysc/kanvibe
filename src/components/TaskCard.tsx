"use client";

import { useRef, useState } from "react";
import { Draggable } from "@hello-pangea/dnd";
import { useLocale } from "next-intl";
import { Link, useRouter } from "@/desktop/renderer/navigation";
import {
  navigateToTaskDetail,
  shouldHandleTaskNavigationClick,
} from "@/desktop/renderer/utils/taskNavigation";
import { TaskStatus, type KanbanTask } from "@/entities/KanbanTask";
import TaskCardContent, { type TaskCardContentProps } from "@/components/TaskCardContent";

interface ContextMenuPosition {
  x: number;
  y: number;
}

interface TaskCardProps extends Omit<TaskCardContentProps, "isLiveSessionPanelOpen"> {
  index: number;
  onContextMenu: (task: KanbanTask, position: ContextMenuPosition) => void;
  vimModeEnabled?: boolean;
}

const LIVE_SESSION_PANEL_OPEN_DELAY_MS = 1_200;

const KANBAN_STATUS_ORDER = [
  TaskStatus.TODO,
  TaskStatus.PROGRESS,
  TaskStatus.PENDING,
  TaskStatus.REVIEW,
  TaskStatus.DONE,
];

const TASK_CARD_SELECTOR = "[data-kanban-task-card='true']";

function getTaskCards() {
  return Array.from(document.querySelectorAll<HTMLAnchorElement>(TASK_CARD_SELECTOR));
}

function getTaskIndex(card: HTMLElement) {
  const index = Number(card.dataset.kanbanIndex);
  return Number.isFinite(index) ? index : 0;
}

function focusTaskCard(card: HTMLAnchorElement) {
  card.focus({ preventScroll: true });
  card.scrollIntoView?.({ block: "nearest", inline: "nearest" });
}

function findTaskCardByStatusAndIndex(status: TaskStatus, index: number) {
  const cards = getTaskCards().filter((card) => card.dataset.kanbanStatus === status);
  if (cards.length === 0) return null;
  const targetIndex = Math.max(0, Math.min(index, cards.length - 1));
  return cards.find((card) => getTaskIndex(card) === targetIndex) ?? cards[targetIndex] ?? null;
}

function findHorizontalTaskCard(currentStatus: TaskStatus, currentIndex: number, direction: -1 | 1) {
  const start = KANBAN_STATUS_ORDER.indexOf(currentStatus);
  if (start === -1) return null;

  for (let index = start + direction; index >= 0 && index < KANBAN_STATUS_ORDER.length; index += direction) {
    const target = findTaskCardByStatusAndIndex(KANBAN_STATUS_ORDER[index], currentIndex);
    if (target) return target;
  }

  return null;
}

function isShiftOnlyKeyboardShortcut(event: React.KeyboardEvent, key: string) {
  return event.key === key && event.shiftKey && !event.altKey && !event.ctrlKey && !event.metaKey;
}

function getTaskFocusNavigationKey(event: React.KeyboardEvent, vimModeEnabled: boolean): "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight" | null {
  if (["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(event.key)) {
    return event.key as "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight";
  }

  if (!vimModeEnabled || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) {
    return null;
  }

  switch (event.key) {
    case "k":
      return "ArrowUp";
    case "j":
      return "ArrowDown";
    case "h":
      return "ArrowLeft";
    case "l":
      return "ArrowRight";
    default:
      return null;
  }
}

export default function TaskCard({
  task,
  index,
  onContextMenu,
  projectName,
  projectColor,
  projectIconDataUrl,
  isBaseProject,
  unreadNotificationCount,
  rootPriorityByProjectId,
  vimModeEnabled = true,
  runningAgentPanes,
  diffStats,
}: TaskCardProps) {
  const [isLiveSessionPanelOpen, setIsLiveSessionPanelOpen] = useState(false);
  const livePanelOpenTimeoutRef = useRef<number | null>(null);
  const cardStyle = projectColor ? { borderColor: projectColor } : undefined;
  const locale = useLocale();
  const router = useRouter();

  /**
   * 카드 사이를 훑고 지나갈 때마다 패널이 번쩍이지 않도록 잠깐 머무른 뒤에 연다.
   * 여는 순간에만 상태를 건드리므로, 스쳐 지나가는 카드는 리렌더도 세션 조회도 일으키지 않는다.
   */
  function handleCardActivate() {
    if (livePanelOpenTimeoutRef.current !== null) {
      return;
    }

    livePanelOpenTimeoutRef.current = window.setTimeout(() => {
      setIsLiveSessionPanelOpen(true);
    }, LIVE_SESSION_PANEL_OPEN_DELAY_MS);
  }

  function handleCardDeactivate() {
    if (livePanelOpenTimeoutRef.current !== null) {
      window.clearTimeout(livePanelOpenTimeoutRef.current);
      livePanelOpenTimeoutRef.current = null;
    }

    setIsLiveSessionPanelOpen(false);
  }

  function handleTaskKeyDown(event: React.KeyboardEvent<HTMLAnchorElement>) {
    if (isShiftOnlyKeyboardShortcut(event, "Enter")) {
      event.preventDefault();
      event.stopPropagation();

      void navigateToTaskDetail(task.id, {
        currentLocale: locale,
        openInNewWindow: true,
      });
      return;
    }

    if (isShiftOnlyKeyboardShortcut(event, "F10")) {
      event.preventDefault();
      event.stopPropagation();

      const rect = event.currentTarget.getBoundingClientRect();
      onContextMenu(task, {
        x: rect.left + 12,
        y: rect.top + 12,
      });
      return;
    }

    const navigationKey = getTaskFocusNavigationKey(event, vimModeEnabled);
    if (!navigationKey) {
      return;
    }

    event.preventDefault();
    event.stopPropagation();

    const currentStatus = event.currentTarget.dataset.kanbanStatus as TaskStatus | undefined;
    if (!currentStatus) return;

    const currentIndex = getTaskIndex(event.currentTarget);
    const target =
      navigationKey === "ArrowUp"
        ? findTaskCardByStatusAndIndex(currentStatus, currentIndex - 1)
        : navigationKey === "ArrowDown"
          ? findTaskCardByStatusAndIndex(currentStatus, currentIndex + 1)
          : findHorizontalTaskCard(currentStatus, currentIndex, navigationKey === "ArrowRight" ? 1 : -1);

    if (target) {
      focusTaskCard(target);
    }
  }

  function handleTaskClick(event: React.MouseEvent<HTMLAnchorElement>) {
    if (!shouldHandleTaskNavigationClick(event)) {
      return;
    }

    event.preventDefault();
    void navigateToTaskDetail(task.id, {
      currentLocale: locale,
      navigate: router.push,
    });
  }

  return (
    <Draggable draggableId={task.id} index={index}>
      {(provided, snapshot) => (
        <Link
          ref={provided.innerRef}
          href={`/task/${task.id}`}
          {...provided.draggableProps}
          {...provided.dragHandleProps}
          role="link"
          data-kanban-task-card="true"
          data-kanban-task-id={task.id}
          data-kanban-status={task.status}
          data-kanban-index={index}
          onClick={handleTaskClick}
          onKeyDown={handleTaskKeyDown}
          onFocus={handleCardActivate}
          onBlur={handleCardDeactivate}
          onMouseEnter={handleCardActivate}
          onMouseLeave={handleCardDeactivate}
          onContextMenu={(e) => {
            e.preventDefault();
            onContextMenu(task, { x: e.clientX, y: e.clientY });
          }}
          style={{
            ...provided.draggableProps.style,
            ...cardStyle,
          }}
          /** 세션 패널은 카드 아래로 넘치므로 열려 있는 동안에는 카드가 잘라내지 않게 한다 */
          className={`group relative mb-1.5 block ${isLiveSessionPanelOpen ? "" : "overflow-hidden"} rounded-md border border-border-subtle px-2.5 py-2 transition-[background-color,border-color,box-shadow] cursor-pointer outline-none focus:border-border-brand focus:bg-bg-surface/90 ${isBaseProject ? "pr-8" : ""} ${
            snapshot.isDragging
              ? "bg-bg-surface shadow-md ring-1 ring-border-brand"
              : "hover:bg-bg-surface/70"
          }`}
        >
          <TaskCardContent
            task={task}
            projectName={projectName}
            projectColor={projectColor}
            projectIconDataUrl={projectIconDataUrl}
            isBaseProject={isBaseProject}
            unreadNotificationCount={unreadNotificationCount}
            rootPriorityByProjectId={rootPriorityByProjectId}
            runningAgentPanes={runningAgentPanes}
            diffStats={diffStats}
            isLiveSessionPanelOpen={isLiveSessionPanelOpen}
          />
        </Link>
      )}
    </Draggable>
  );
}
