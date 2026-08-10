"use client";

import { useRef, useState } from "react";
import { Draggable } from "@hello-pangea/dnd";
import { useLocale, useTranslations } from "next-intl";
import { Link, useRouter } from "@/desktop/renderer/navigation";
import {
  navigateToTaskDetail,
  shouldHandleTaskNavigationClick,
} from "@/desktop/renderer/utils/taskNavigation";
import { TaskStatus, type KanbanTask } from "@/entities/KanbanTask";
import { TaskPriority } from "@/entities/TaskPriority";
import {
  isInheritedPriority,
  resolveEffectivePriority,
} from "@/desktop/renderer/utils/boardTaskSort";
import ProjectIcon from "@/components/ProjectIcon";
import { TaskCardLiveSessions } from "@/components/TaskCardLiveSessions";
import type { RunningAgentPane } from "@/lib/aiSessions/types";

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
  projectIconDataUrl?: string | null;
  isBaseProject?: boolean;
  unreadNotificationCount?: number;
  /** projectId → 프로젝트 root task의 우선순위. task가 자기 우선순위를 갖지 않을 때 이 값을 물려받는다 */
  rootPriorityByProjectId?: Map<string, TaskPriority>;
  vimModeEnabled?: boolean;
  /** 보드 전체가 한 번만 조회한 실행중 에이전트 목록 */
  runningAgentPanes?: RunningAgentPane[];
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

const EMPTY_ROOT_PRIORITY_MAP: Map<string, TaskPriority> = new Map();
const EMPTY_RUNNING_AGENT_PANES: RunningAgentPane[] = [];
const LIVE_SESSION_PANEL_OPEN_DELAY_MS = 1_200;

const badgeClassName = "inline-flex items-center rounded border border-border-subtle px-1.5 py-0.5 text-[10px]";
/** 프로젝트 마커 열 + 본문 열. 마커 폭은 ProjectIcon 기본 크기(h-3.5 w-3.5 = 14px)와 맞춘다 */
const PROJECT_MARKER_GRID_COLUMNS = "grid-cols-[14px_minmax(0,1fr)]";

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

function BellIcon({ size = 11 }: { size?: number }) {
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
      <path d="M15 17h5l-1.4-1.4A2 2 0 0 1 18 14.2V11a6 6 0 1 0-12 0v3.2a2 2 0 0 1-.6 1.4L4 17h5" />
      <path d="M9 17a3 3 0 0 0 6 0" />
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
  unreadNotificationCount = 0,
  rootPriorityByProjectId = EMPTY_ROOT_PRIORITY_MAP,
  vimModeEnabled = true,
  runningAgentPanes = EMPTY_RUNNING_AGENT_PANES,
}: TaskCardProps) {
  const [isLiveSessionPanelOpen, setIsLiveSessionPanelOpen] = useState(false);
  const livePanelOpenTimeoutRef = useRef<number | null>(null);
  const cardStyle = projectColor ? { borderColor: projectColor } : undefined;
  const locale = useLocale();
  const t = useTranslations("task");
  const tc = useTranslations("common");
  const router = useRouter();
  const effectivePriority = resolveEffectivePriority(task, rootPriorityByProjectId);
  const isPriorityInherited = isInheritedPriority(task, rootPriorityByProjectId);

  /** 프로젝트명 행에 겹쳐 둔다. 프로젝트가 없는 카드에서는 첫 줄인 제목 행으로 내려간다 */
  const unreadBadge = unreadNotificationCount > 0 ? (
    <span
      className={`${badgeClassName} absolute right-0 top-1/2 h-5 -translate-y-1/2 gap-1 border-transparent bg-brand-primary font-semibold text-text-inverse`}
      aria-label={tc("unreadCount", { count: unreadNotificationCount })}
      data-testid="unread-notification-badge"
    >
      <BellIcon />
      {unreadNotificationCount}
    </span>
  ) : null;

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
            <div className={`mb-1 grid ${PROJECT_MARKER_GRID_COLUMNS} items-center gap-2`}>
              <ProjectIcon
                projectName={projectName}
                iconDataUrl={projectIconDataUrl}
                color={projectColor}
              />
              <div className="relative min-w-0">
                <span
                  className="block truncate text-xs font-semibold leading-4"
                  style={{ color: projectColor }}
                >
                  {projectName}
                </span>

                {unreadBadge}
              </div>
            </div>
          )}

          <div className={projectName ? `grid ${PROJECT_MARKER_GRID_COLUMNS} items-start gap-2` : "block"}>
            {projectName ? <span aria-hidden="true" /> : null}
            <div className="min-w-0">
              <div className="relative">
                <h3 className="truncate text-[13px] font-medium leading-5 text-text-primary">
                  {task.title}
                </h3>

                {!projectName && unreadBadge}
              </div>

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

            <TaskCardLiveSessions
              taskId={task.id}
              worktreePath={task.worktreePath}
              runningPanes={runningAgentPanes}
              isPanelOpen={isLiveSessionPanelOpen}
            />

            {task.sshHost && (
              <span className={`${badgeClassName} min-w-0 truncate bg-tag-ssh-bg text-tag-ssh-text`}>
                {task.sshHost}
              </span>
            )}

            {effectivePriority && (
              <span
                data-testid="task-priority-badge"
                data-inherited-priority={isPriorityInherited}
                title={isPriorityInherited ? t("inheritedPriority") : undefined}
                className={`ml-auto rounded px-1.5 py-0.5 text-[10px] font-semibold ${priorityConfig[effectivePriority].colorClass} ${
                  isPriorityInherited
                    ? "border border-dashed border-border-default opacity-70"
                    : "border border-border-subtle"
                }`}
              >
                {priorityConfig[effectivePriority].label}
              </span>
            )}
          </div>
        </Link>
      )}
    </Draggable>
  );
}
