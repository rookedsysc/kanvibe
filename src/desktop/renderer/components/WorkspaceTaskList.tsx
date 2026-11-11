"use client";

import { useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { useLocale, useTranslations } from "next-intl";
import { computeProjectColor } from "@/lib/projectColor";
import { buildProjectDisplayMap } from "@/desktop/renderer/utils/projectDisplay";
import TaskCardContent from "@/components/TaskCardContent";
import { useHasBoardShortcutBlocker } from "@/desktop/renderer/components/BoardCommandProvider";
import { createWorkspaceListFocusTracker, getCurrentShortcutPlatform } from "@/desktop/shared/keyboardShortcut";
import { isShortcutCaptureActive } from "@/desktop/renderer/utils/shortcutBindings";
import { requestActiveTerminalFocusAfterUiSettles } from "@/desktop/renderer/utils/terminalFocus";
import { useRunningAgentPanes } from "@/desktop/renderer/hooks/useLiveAiSessions";
import { useBoardTaskDiffStats } from "@/desktop/renderer/hooks/useTaskDiffStats";
import { useUnreadNotificationCountByTask } from "@/desktop/renderer/hooks/useUnreadNotificationCountByTask";
import BoardSortPicker from "@/components/BoardSortPicker";
import ProjectSelector from "@/components/ProjectSelector";
import { useBoardSortPreference } from "@/desktop/renderer/hooks/useBoardSortPreference";
import { useProjectFilterParams } from "@/desktop/renderer/hooks/useProjectFilterParams";
import { TASK_KIND_FILTER_VALUES, useTaskKindFilterParams } from "@/desktop/renderer/hooks/useTaskKindFilterParams";
import {
  buildProjectFilterSet,
  buildProjectRootPriorityMap,
  matchesProjectFilter,
  matchesTaskKindFilter,
  sortTasksForBoard,
} from "@/desktop/renderer/utils/boardTaskSort";
import { TASK_STATUS_ORDER, type KanbanTask, type TaskStatus } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";

interface WorkspaceTaskListProps {
  tasks: KanbanTask[];
  projects: Project[];
  selectedTaskId: string | null;
  selectionRequestKey?: string;
  onSelectTask: (taskId: string) => void;
  hasMoreDoneTasks: boolean;
  onLoadMoreDoneTasks: () => void;
}

const STATUS_DOT_CLASS_NAMES: Record<TaskStatus, string> = {
  todo: "bg-status-todo",
  progress: "bg-status-progress",
  pending: "bg-status-pending",
  review: "bg-status-review",
  done: "bg-status-done",
};

const FOCUS_RING_CLASS_NAME = "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-primary";

function matchesSearchQuery(task: KanbanTask, normalizedQuery: string, projectNameById: Map<string, string>): boolean {
  if (!normalizedQuery) return true;

  const projectName = task.projectId ? projectNameById.get(task.projectId) ?? "" : "";
  const searchableText = `${task.title} ${task.branchName ?? ""} ${projectName}`.toLowerCase();
  return searchableText.includes(normalizedQuery);
}

/** 워크스페이스 왼쪽 열. task를 상태 순서로 나열하고 검색·프로젝트·종류·상태 필터와 보드 정렬을 함께 적용한다 */
export default function WorkspaceTaskList({
  tasks,
  projects,
  selectedTaskId,
  selectionRequestKey,
  onSelectTask,
  hasMoreDoneTasks,
  onLoadMoreDoneTasks,
}: WorkspaceTaskListProps) {
  const t = useTranslations("workspace");
  const tBoard = useTranslations("board");
  const tTask = useTranslations("task");
  const locale = useLocale();
  const listRef = useRef<HTMLUListElement>(null);
  const [hasListFocus, setHasListFocus] = useState(false);
  const [hiddenSelectionId, setHiddenSelectionId] = useState<string | null>(null);
  const hasShortcutBlocker = useHasBoardShortcutBlocker();
  const runningAgentPanes = useRunningAgentPanes(true);
  const unreadCountByTask = useUnreadNotificationCountByTask();
  const [searchQuery, setSearchQuery] = useState("");
  /** 비어 있으면 모든 상태를 보여 준다 */
  const [selectedStatuses, setSelectedStatuses] = useState<ReadonlySet<TaskStatus>>(() => new Set());
  const [sortPreference, setSortPreference] = useBoardSortPreference();
  const [taskKindFilter, setTaskKindFilter] = useTaskKindFilterParams();
  const filterableProjects = useMemo(() => projects.filter((project) => !project.isWorktree), [projects]);
  const filterableProjectIds = useMemo(() => filterableProjects.map((project) => project.id), [filterableProjects]);
  const [selectedProjectIds, setSelectedProjectIds] = useProjectFilterParams(filterableProjectIds);

  const projectLookup = useMemo(() => new Map(projects.map((project) => [project.id, project])), [projects]);
  const projectDisplayMap = useMemo(() => buildProjectDisplayMap(projects), [projects]);
  const projectNameById = useMemo(() => new Map([...projectDisplayMap].map(([id, display]) => [id, display.name])), [projectDisplayMap]);
  const projectFilterSet = useMemo(
    () => buildProjectFilterSet(selectedProjectIds, projects),
    [selectedProjectIds, projects],
  );
  const sortContext = useMemo(() => ({
    rootPriorityByProjectId: buildProjectRootPriorityMap(tasks, projectLookup),
    projectNameById,
    locale,
  }), [locale, projectLookup, projectNameById, tasks]);

  const filteredTasks = useMemo(() => {
    const normalizedQuery = searchQuery.trim().toLowerCase();
    const visibleStatuses = TASK_STATUS_ORDER.filter((status) => selectedStatuses.size === 0 || selectedStatuses.has(status));

    return visibleStatuses.flatMap((status) => {
      const matchingTasks = tasks.filter((task) => (
        task.status === status
        && matchesProjectFilter(task, projectFilterSet)
        && matchesTaskKindFilter(task, taskKindFilter, projectLookup)
        && matchesSearchQuery(task, normalizedQuery, projectNameById)
      ));
      return sortTasksForBoard(matchingTasks, sortPreference, sortContext);
    });
  }, [projectFilterSet, projectLookup, projectNameById, searchQuery, selectedStatuses, sortContext, sortPreference, taskKindFilter, tasks]);

  const listedTasks = useMemo(() => {
    const selectedTask = tasks.find((task) => task.id === selectedTaskId);
    return selectedTask && hiddenSelectionId !== selectedTaskId && !filteredTasks.some((task) => task.id === selectedTaskId)
      ? [selectedTask, ...filteredTasks]
      : filteredTasks;
  }, [filteredTasks, hiddenSelectionId, selectedTaskId, tasks]);
  useEffect(() => {
    if (selectionRequestKey) setHiddenSelectionId(null);
  }, [selectionRequestKey]);
  const hasListedSelection = listedTasks.some((task) => task.id === selectedTaskId);
  const diffStatsByTaskId = useBoardTaskDiffStats(listedTasks.map((task) => task.id), true);

  useEffect(() => {
    const selected = Array.from(listRef.current?.querySelectorAll<HTMLElement>("[data-workspace-task-id]") ?? [])
      .find((card) => card.dataset.workspaceTaskId === selectedTaskId);
    selected?.scrollIntoView?.({ block: "nearest" });
  }, [listedTasks, selectedTaskId]);

  useEffect(() => {
    const tracker = createWorkspaceListFocusTracker(getCurrentShortcutPlatform());
    const focusSelectedTask = () => {
      if (hasShortcutBlocker || isShortcutCaptureActive() || document.querySelector('[role="dialog"], [aria-modal="true"]')) return;
      const cards = Array.from(listRef.current?.querySelectorAll<HTMLElement>("[data-workspace-task-id]") ?? []);
      const selected = cards.find((card) => card.dataset.workspaceTaskId === selectedTaskId) ?? cards[0];
      if (selected?.dataset.workspaceTaskId && selected.dataset.workspaceTaskId !== selectedTaskId) {
        onSelectTask(selected.dataset.workspaceTaskId);
      }
      selected?.focus({ preventScroll: true });
      selected?.scrollIntoView?.({ block: "nearest" });
    };
    const unsubscribe = window.kanvibeDesktop?.onWorkspaceListFocusShortcut?.(focusSelectedTask);
    const handleKey = (event: KeyboardEvent) => {
      if (hasShortcutBlocker || isShortcutCaptureActive()) {
        tracker.reset();
        return;
      }
      if (tracker.handleKeyboardEvent(event)) focusSelectedTask();
    };
    if (!unsubscribe) {
      window.addEventListener("keydown", handleKey, true);
      window.addEventListener("keyup", handleKey, true);
    }
    window.addEventListener("blur", tracker.reset);
    return () => {
      unsubscribe?.();
      window.removeEventListener("keydown", handleKey, true);
      window.removeEventListener("keyup", handleKey, true);
      window.removeEventListener("blur", tracker.reset);
    };
  }, [hasShortcutBlocker, onSelectTask, selectedTaskId]);

  function handleTaskKeyDown(event: ReactKeyboardEvent<HTMLDivElement>, index: number) {
    if (event.isDefaultPrevented() || event.nativeEvent.isComposing || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (event.key === "Enter") {
      event.preventDefault();
      onSelectTask(listedTasks[index].id);
      setHasListFocus(false);
      requestActiveTerminalFocusAfterUiSettles();
      return;
    }
    if (event.key === " ") {
      event.preventDefault();
      onSelectTask(listedTasks[index].id);
      return;
    }
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    const nextIndex = Math.max(0, Math.min(index + (event.key === "ArrowUp" ? -1 : 1), listedTasks.length - 1));
    const nextTask = listedTasks[nextIndex];
    if (!nextTask) return;
    onSelectTask(nextTask.id);
    listRef.current?.querySelectorAll<HTMLElement>("[data-workspace-task-id]")[nextIndex]?.focus({ preventScroll: true });
  }

  function toggleStatus(status: TaskStatus) {
    setHiddenSelectionId(selectedTaskId);
    setSelectedStatuses((current) => {
      const next = new Set(current);
      if (next.has(status)) {
        next.delete(status);
      } else {
        next.add(status);
      }
      return next;
    });
  }

  return (
    <aside
      data-terminal-focus-blocker={hasListFocus ? "true" : undefined}
      onFocusCapture={() => setHasListFocus(true)}
      onBlurCapture={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setHasListFocus(false);
      }}
      className="flex h-full w-80 shrink-0 flex-col border-r border-border-default bg-bg-surface"
    >
      <div className="space-y-2 border-b border-border-default p-3">
        <input
          type="search"
          value={searchQuery}
          onChange={(event) => { setHiddenSelectionId(selectedTaskId); setSearchQuery(event.target.value); }}
          placeholder={t("searchPlaceholder")}
          aria-label={t("searchPlaceholder")}
          className="h-8 w-full rounded-md border border-border-default bg-bg-page px-2.5 text-sm text-text-primary placeholder:text-text-muted focus:border-brand-primary focus:outline-none"
        />
        <div className="flex items-center gap-2">
          <div
            role="group"
            aria-label={tBoard("taskKindFilter.label")}
            className="flex h-8 flex-1 items-stretch rounded-md border border-border-default bg-bg-page p-0.5"
          >
            {TASK_KIND_FILTER_VALUES.map((filter) => {
              const isActive = taskKindFilter === filter;
              return (
                <button
                  key={filter}
                  type="button"
                  aria-pressed={isActive}
                  title={tBoard(`taskKindFilter.descriptions.${filter}`)}
                  onClick={() => { setHiddenSelectionId(selectedTaskId); setTaskKindFilter(filter); }}
                  className={`flex flex-1 items-center justify-center rounded text-xs font-medium transition-colors ${FOCUS_RING_CLASS_NAME} ${
                    isActive
                      ? "bg-brand-primary text-text-inverse"
                      : "text-text-secondary hover:bg-bg-surface hover:text-text-primary"
                  }`}
                >
                  {tBoard(`taskKindFilter.options.${filter}`)}
                </button>
              );
            })}
          </div>
          <BoardSortPicker preference={sortPreference} onChange={(preference) => { setHiddenSelectionId(selectedTaskId); setSortPreference(preference); }} />
        </div>
        <ProjectSelector
          multiple
          projects={filterableProjects}
          selectedProjectIds={selectedProjectIds}
          onSelectionChange={(ids) => { setHiddenSelectionId(selectedTaskId); setSelectedProjectIds(ids); }}
          placeholder={tBoard("allProjects")}
          searchPlaceholder={tTask("projectSearch")}
          compact
        />
        <div role="group" aria-label={t("statusFilterLabel")} className="flex flex-wrap gap-1">
          {TASK_STATUS_ORDER.map((status) => {
            const isActive = selectedStatuses.has(status);
            return (
              <button
                key={status}
                type="button"
                aria-pressed={isActive}
                onClick={() => toggleStatus(status)}
                className={`flex items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium transition-colors ${FOCUS_RING_CLASS_NAME} ${
                  isActive
                    ? "bg-brand-subtle text-text-primary ring-1 ring-inset ring-border-brand"
                    : "text-text-secondary hover:bg-bg-page hover:text-text-primary"
                }`}
              >
                <span aria-hidden="true" className={`h-2 w-2 rounded-full ${STATUS_DOT_CLASS_NAMES[status]}`} />
                {tBoard(`columns.${status}`)}
              </button>
            );
          })}
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-1.5">
        <ul ref={listRef} aria-label={t("taskListLabel")} className="space-y-1.5">
          {listedTasks.map((task, index) => {
            const isSelected = task.id === selectedTaskId;
            const display = task.projectId ? projectDisplayMap.get(task.projectId) : undefined;
            const project = display?.project;
            const isBaseProject = Boolean(task.worktreePath && !task.worktreePath.includes("__worktrees"));
            return (
              <li key={task.id}>
                <div
                  role="button"
                  tabIndex={isSelected || (!hasListedSelection && index === 0) ? 0 : -1}
                  aria-current={isSelected ? "true" : undefined}
                  aria-description={tBoard(`columns.${task.status}`)}
                  data-workspace-task-id={task.id}
                  onClick={() => onSelectTask(task.id)}
                  onKeyDown={(event) => handleTaskKeyDown(event, index)}
                  className={`relative w-full cursor-pointer rounded-md px-2.5 py-2 text-left transition-colors ${FOCUS_RING_CLASS_NAME} ${isBaseProject ? "pr-8" : ""} ${
                    isSelected ? "bg-brand-subtle" : "hover:bg-bg-page"
                  }`}
                >
                  <TaskCardContent
                    task={task}
                    projectName={display?.name}
                    projectColor={display ? project?.color || computeProjectColor(display.name) : undefined}
                    projectIconDataUrl={project?.iconDataUrl}
                    isBaseProject={isBaseProject}
                    rootPriorityByProjectId={sortContext.rootPriorityByProjectId}
                    runningAgentPanes={runningAgentPanes}
                    unreadNotificationCount={unreadCountByTask[task.id] ?? 0}
                    diffStats={diffStatsByTaskId[task.id]}
                  />
                </div>
              </li>
            );
          })}
        </ul>
        {listedTasks.length === 0 ? (
          <p className="px-2.5 py-6 text-sm text-text-muted">
            {tasks.length === 0 ? t("emptyTasks") : t("noMatchingTasks")}
          </p>
        ) : null}
        {hasMoreDoneTasks ? (
          <button
            type="button"
            onClick={onLoadMoreDoneTasks}
            className={`mt-1 w-full rounded-md px-2.5 py-2 text-left text-xs font-medium text-brand-primary hover:bg-bg-page ${FOCUS_RING_CLASS_NAME}`}
          >
            {t("loadMoreDone")}
          </button>
        ) : null}
      </div>
    </aside>
  );
}
