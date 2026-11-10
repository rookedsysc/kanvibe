"use client";

import { useMemo, useState } from "react";
import { useLocale, useTranslations } from "next-intl";
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
  onSelectTask,
  hasMoreDoneTasks,
  onLoadMoreDoneTasks,
}: WorkspaceTaskListProps) {
  const t = useTranslations("workspace");
  const tBoard = useTranslations("board");
  const tTask = useTranslations("task");
  const locale = useLocale();
  const [searchQuery, setSearchQuery] = useState("");
  /** 비어 있으면 모든 상태를 보여 준다 */
  const [selectedStatuses, setSelectedStatuses] = useState<ReadonlySet<TaskStatus>>(() => new Set());
  const [sortPreference, setSortPreference] = useBoardSortPreference();
  const [taskKindFilter, setTaskKindFilter] = useTaskKindFilterParams();
  const filterableProjects = useMemo(() => projects.filter((project) => !project.isWorktree), [projects]);
  const filterableProjectIds = useMemo(() => filterableProjects.map((project) => project.id), [filterableProjects]);
  const [selectedProjectIds, setSelectedProjectIds] = useProjectFilterParams(filterableProjectIds);

  const projectLookup = useMemo(() => new Map(projects.map((project) => [project.id, project])), [projects]);
  const projectNameById = useMemo(() => new Map(projects.map((project) => [project.id, project.name])), [projects]);
  const projectFilterSet = useMemo(
    () => buildProjectFilterSet(selectedProjectIds, projects),
    [selectedProjectIds, projects],
  );
  const sortContext = useMemo(() => ({
    rootPriorityByProjectId: buildProjectRootPriorityMap(tasks, projectLookup),
    projectNameById,
    locale,
  }), [locale, projectLookup, projectNameById, tasks]);

  const listedTasks = useMemo(() => {
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

  function toggleStatus(status: TaskStatus) {
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
    <aside className="flex h-full w-80 shrink-0 flex-col border-r border-border-default bg-bg-surface">
      <div className="space-y-2 border-b border-border-default p-3">
        <input
          type="search"
          value={searchQuery}
          onChange={(event) => setSearchQuery(event.target.value)}
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
                  onClick={() => setTaskKindFilter(filter)}
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
          <BoardSortPicker preference={sortPreference} onChange={setSortPreference} />
        </div>
        <ProjectSelector
          multiple
          projects={filterableProjects}
          selectedProjectIds={selectedProjectIds}
          onSelectionChange={setSelectedProjectIds}
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
        <ul aria-label={t("taskListLabel")} className="space-y-0.5">
          {listedTasks.map((task) => {
            const isSelected = task.id === selectedTaskId;
            const projectName = task.projectId ? projectNameById.get(task.projectId) : null;
            return (
              <li key={task.id}>
                <button
                  type="button"
                  aria-current={isSelected ? "true" : undefined}
                  onClick={() => onSelectTask(task.id)}
                  className={`flex w-full items-start gap-2.5 rounded-md px-2.5 py-2 text-left transition-colors ${FOCUS_RING_CLASS_NAME} ${
                    isSelected ? "bg-brand-subtle" : "hover:bg-bg-page"
                  }`}
                >
                  <span
                    aria-hidden="true"
                    title={tBoard(`columns.${task.status}`)}
                    className={`mt-1.5 h-2 w-2 shrink-0 rounded-full ${STATUS_DOT_CLASS_NAMES[task.status]}`}
                  />
                  <span className="min-w-0 flex-1">
                    <span data-testid="workspace-task-title" className="block truncate text-sm font-medium text-text-primary">
                      {task.title}
                    </span>
                    <span className="block truncate text-xs text-text-muted">
                      {[projectName, task.branchName].filter(Boolean).join(" · ")}
                    </span>
                  </span>
                </button>
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
