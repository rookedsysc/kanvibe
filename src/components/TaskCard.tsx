"use client";

import { Draggable } from "@hello-pangea/dnd";
import { useLocale } from "next-intl";
import { Link, useRouter } from "@/desktop/renderer/navigation";
import {
  navigateToTaskDetail,
  shouldHandleTaskNavigationClick,
} from "@/desktop/renderer/utils/taskNavigation";
import { TaskStatus, type KanbanTask } from "@/entities/KanbanTask";
import { TaskPriority } from "@/entities/TaskPriority";

interface ContextMenuPosition {
  x: number;
  y: number;
}

interface TaskCardProps {
  task: KanbanTask;
  index: number;
  onContextMenu: (task: KanbanTask, position: ContextMenuPosition) => void;
  projectName?: string;
  projectColor?: string;
  isBaseProject?: boolean;
}

const agentTagColors: Record<string, string> = {
  claude: "bg-tag-claude-bg text-tag-claude-text",
  gemini: "bg-tag-gemini-bg text-tag-gemini-text",
  codex: "bg-tag-codex-bg text-tag-codex-text",
};

const priorityConfig: Record<TaskPriority, { label: string; colorClass: string }> = {
  [TaskPriority.LOW]: { label: "P3", colorClass: "bg-priority-low-bg text-priority-low-text" },
  [TaskPriority.MEDIUM]: { label: "P2", colorClass: "bg-priority-medium-bg text-priority-medium-text" },
  [TaskPriority.HIGH]: { label: "P1", colorClass: "bg-priority-high-bg text-priority-high-text" },
};

const badgeClassName = "inline-flex items-center rounded border border-border-subtle px-1.5 py-0.5 text-[10px]";

const KANBAN_STATUS_ORDER = [
  TaskStatus.TODO,
  TaskStatus.PROGRESS,
  TaskStatus.PENDING,
  TaskStatus.REVIEW,
  TaskStatus.DONE,
];

const TASK_CARD_SELECTOR = "[data-kanban-task-card='true']";

function CrownIcon() {
  return (
    <svg
      width="12"
      height="12"
      viewBox="0 0 16 16"
      fill="none"
      aria-hidden="true"
      data-icon-name="CrownIcon"
    >
      <path
        d="M2.2 5.1 5.3 8l2.7-4.1L10.7 8l3.1-2.9-1.2 6.6H3.4L2.2 5.1Z"
        fill="currentColor"
      />
      <path d="M3.7 13h8.6" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
    </svg>
  );
}

function PullRequestIcon({ size = 12 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <circle cx="6" cy="18" r="3" />
      <circle cx="18" cy="6" r="3" />
      <path d="M6 15V6" />
      <path d="M18 9v1.5A5.5 5.5 0 0 1 12.5 16H9" />
      <path d="m12 13-3 3 3 3" />
    </svg>
  );
}

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

export default function TaskCard({ task, index, onContextMenu, projectName, projectColor, isBaseProject }: TaskCardProps) {
  const cardStyle = projectColor ? { borderColor: projectColor } : undefined;
  const locale = useLocale();
  const router = useRouter();

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

    if (!["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(event.key)) {
      return;
    }

    event.preventDefault();
    event.stopPropagation();

    const currentStatus = event.currentTarget.dataset.kanbanStatus as TaskStatus | undefined;
    if (!currentStatus) return;

    const currentIndex = getTaskIndex(event.currentTarget);
    const target =
      event.key === "ArrowUp"
        ? findTaskCardByStatusAndIndex(currentStatus, currentIndex - 1)
        : event.key === "ArrowDown"
          ? findTaskCardByStatusAndIndex(currentStatus, currentIndex + 1)
          : findHorizontalTaskCard(currentStatus, currentIndex, event.key === "ArrowRight" ? 1 : -1);

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
          onContextMenu={(e) => {
            e.preventDefault();
            onContextMenu(task, { x: e.clientX, y: e.clientY });
          }}
          style={{
            ...provided.draggableProps.style,
            ...cardStyle,
          }}
          className={`group relative mb-1.5 block overflow-hidden rounded-md border border-border-subtle px-2.5 py-2 transition-[background-color,border-color,box-shadow] cursor-pointer outline-none focus:border-border-brand focus:bg-bg-surface/90 ${isBaseProject ? "pr-8" : ""} ${
            snapshot.isDragging
              ? "bg-bg-surface shadow-md ring-1 ring-border-brand"
              : "hover:bg-bg-surface/70"
          }`}
        >
          {/* Base branch 표시 */}
          {isBaseProject && (
            <span
              className="pointer-events-none absolute right-2 top-2 inline-flex h-5 w-5 items-center justify-center rounded-full border border-border-subtle bg-tag-base-bg text-tag-base-text shadow-sm"
              title="Base branch"
              aria-hidden="true"
              data-testid="base-branch-icon"
            >
              <CrownIcon />
            </span>
          )}

          {projectName && (
            <div className="mb-1 grid grid-cols-[6px_minmax(0,1fr)] items-center gap-2">
              <span
                className="h-1.5 w-1.5 shrink-0 rounded-full"
                style={{ backgroundColor: projectColor }}
              />
              <span
                className="truncate text-xs font-semibold leading-4"
                style={{ color: projectColor }}
              >
                {projectName}
              </span>
            </div>
          )}

          <div className={projectName ? "grid grid-cols-[6px_minmax(0,1fr)] items-start gap-2" : "block"}>
            {projectName ? <span aria-hidden="true" /> : null}
            <div className="min-w-0">
              <h3 className="truncate text-[13px] font-medium leading-5 text-text-primary">
                {task.title}
              </h3>

              {task.description && (
                <p className="mt-0.5 line-clamp-1 text-[11px] leading-4 text-text-muted">
                  {task.description}
                </p>
              )}
            </div>
          </div>

          <div className="mt-1.5 flex items-center gap-1.5 overflow-hidden">
            {task.prUrl && (
              <span
                role="link"
                onClick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  window.open(task.prUrl!, "_blank", "noopener,noreferrer");
                }}
                className={`${badgeClassName} gap-1 bg-tag-pr-bg text-tag-pr-text transition-opacity hover:opacity-80`}
              >
                <PullRequestIcon />
                PR
              </span>
            )}

            {task.agentType && (
              <span
                className={`rounded border border-border-subtle px-1.5 py-0.5 text-[10px] ${
                  agentTagColors[task.agentType] || "bg-tag-neutral-bg text-tag-neutral-text"
                }`}
              >
                {task.agentType}
              </span>
            )}

            {task.sessionType && (
              <span className={`${badgeClassName} bg-tag-session-bg text-tag-session-text`}>
                {task.sessionType}
              </span>
            )}

            {task.sshHost && (
              <span className={`${badgeClassName} bg-tag-ssh-bg text-tag-ssh-text`}>
                {task.sshHost}
              </span>
            )}

            {task.priority && (
              <span
                className={`ml-auto rounded border border-border-subtle px-1.5 py-0.5 text-[10px] font-semibold ${priorityConfig[task.priority].colorClass}`}
              >
                {priorityConfig[task.priority].label}
              </span>
            )}
          </div>
        </Link>
      )}
    </Draggable>
  );
}
