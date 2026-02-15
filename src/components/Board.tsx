"use client";

import { useState, useCallback, useEffect, useMemo } from "react";
import { useTranslations } from "next-intl";
import { DragDropContext, type DropResult } from "@hello-pangea/dnd";
import Column from "./Column";
import CreateTaskModal from "./CreateTaskModal";
import ProjectSelector from "./ProjectSelector";
import ProjectSettings from "./ProjectSettings";
import TaskContextMenu from "./TaskContextMenu";
import BranchTaskModal from "./BranchTaskModal";
import { reorderTasks, deleteTask, getMoreDoneTasks, moveTaskToColumn } from "@/app/actions/kanban";
import type { TasksByStatus } from "@/app/actions/kanban";
import { TaskStatus, type KanbanTask } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";
import { logoutAction } from "@/app/actions/auth";
import { useAutoRefresh } from "@/hooks/useAutoRefresh";

interface BoardProps {
  initialTasks: TasksByStatus;
  initialDoneTotal: number;
  initialDoneLimit: number;
  sshHosts: string[];
  projects: Project[];
  sidebarDefaultCollapsed: boolean;
}

const COLUMNS: { status: TaskStatus; labelKey: string; colorClass: string }[] = [
  { status: TaskStatus.TODO, labelKey: "todo", colorClass: "bg-status-todo" },
  { status: TaskStatus.PROGRESS, labelKey: "progress", colorClass: "bg-status-progress" },
  { status: TaskStatus.REVIEW, labelKey: "review", colorClass: "bg-status-review" },
  { status: TaskStatus.DONE, labelKey: "done", colorClass: "bg-status-done" },
];

const FILTER_STORAGE_KEY = "kanvibe:projectFilter";

interface ContextMenuState {
  isOpen: boolean;
  x: number;
  y: number;
  task: KanbanTask | null;
}

/** worktree repoPath에서 메인 프로젝트 경로를 추출한다 */
function extractMainRepoPath(repoPath: string): string | null {
  const worktreeIndex = repoPath.indexOf("__worktrees");
  if (worktreeIndex === -1) return null;
  return repoPath.slice(0, worktreeIndex);
}

/**
 * 필터된 인덱스를 전체 배열의 올바른 위치에 매핑하여 태스크를 삽입한다.
 * 프로젝트 필터가 활성화된 상태에서 드래그 인덱스가 필터된 리스트 기준이므로,
 * 전체 배열에서의 정확한 삽입 위치를 계산해야 한다.
 */
function insertAtFilteredIndex(
  fullArray: KanbanTask[],
  task: KanbanTask,
  filteredIndex: number,
  filterSet: Set<string> | null
): KanbanTask[] {
  const arr = [...fullArray];

  if (!filterSet) {
    arr.splice(filteredIndex, 0, task);
    return arr;
  }

  const filtered = arr.filter((t) => t.projectId && filterSet.has(t.projectId));

  if (filteredIndex < filtered.length) {
    const targetTask = filtered[filteredIndex];
    const fullIndex = arr.findIndex((t) => t.id === targetTask.id);
    arr.splice(fullIndex, 0, task);
  } else if (filtered.length > 0) {
    const lastTask = filtered[filtered.length - 1];
    const lastIndex = arr.findIndex((t) => t.id === lastTask.id);
    arr.splice(lastIndex + 1, 0, task);
  } else {
    arr.push(task);
  }

  return arr;
}

export default function Board({ initialTasks, initialDoneTotal, initialDoneLimit, sshHosts, projects, sidebarDefaultCollapsed }: BoardProps) {
  useAutoRefresh();
  const t = useTranslations("board");
  const tt = useTranslations("task");
  const tc = useTranslations("common");
  const [tasks, setTasks] = useState<TasksByStatus>(initialTasks);
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [isBranchModalOpen, setIsBranchModalOpen] = useState(false);
  const [isMounted, setIsMounted] = useState(false);
  const [selectedProjectIds, setSelectedProjectIds] = useState<string[]>([]);
  const [doneTotal, setDoneTotal] = useState(initialDoneTotal);
  const [doneOffset, setDoneOffset] = useState(initialDoneLimit);
  const [isLoadingMore, setIsLoadingMore] = useState(false);

  /** projectId → 표시할 프로젝트 이름 매핑. worktree 프로젝트는 메인 프로젝트 이름으로 resolve한다 */
  const projectNameMap = useMemo(() => {
    const nameMap: Record<string, string> = {};
    const pathToName: Record<string, string> = {};

    for (const project of projects) {
      const mainPath = extractMainRepoPath(project.repoPath);
      if (!mainPath) {
        pathToName[project.repoPath] = project.name;
      }
    }

    for (const project of projects) {
      const mainPath = extractMainRepoPath(project.repoPath);
      if (mainPath && pathToName[mainPath]) {
        nameMap[project.id] = pathToName[mainPath];
      } else if (mainPath) {
        const baseName = mainPath.split("/").pop() || project.name;
        nameMap[project.id] = baseName;
      } else {
        nameMap[project.id] = project.name;
      }
    }

    return nameMap;
  }, [projects]);

  /** 필터 드롭다운에 표시할 메인 프로젝트 목록 (worktree 제외) */
  const filterableProjects = useMemo(
    () => projects.filter((p) => !p.isWorktree),
    [projects]
  );

  /** 선택된 프로젝트 + worktree 프로젝트 ID 집합. null이면 전체 표시 */
  const projectFilterSet = useMemo(() => {
    if (selectedProjectIds.length === 0) return null;

    const matchingIds = new Set<string>();

    for (const id of selectedProjectIds) {
      const mainProject = projects.find((p) => p.id === id);
      if (!mainProject) continue;

      matchingIds.add(mainProject.id);
      for (const p of projects) {
        if (p.repoPath.startsWith(mainProject.repoPath + "__worktrees")) {
          matchingIds.add(p.id);
        }
      }
    }

    return matchingIds.size > 0 ? matchingIds : null;
  }, [selectedProjectIds, projects]);

  /** 프로젝트 필터가 적용된 태스크 목록 */
  const filteredTasks = useMemo(() => {
    if (!projectFilterSet) return tasks;

    const filtered: TasksByStatus = {
      [TaskStatus.TODO]: [],
      [TaskStatus.PROGRESS]: [],
      [TaskStatus.REVIEW]: [],
      [TaskStatus.DONE]: [],
    };

    for (const status of Object.values(TaskStatus)) {
      filtered[status] = tasks[status].filter(
        (task) => task.projectId && projectFilterSet.has(task.projectId)
      );
    }

    return filtered;
  }, [tasks, projectFilterSet]);

  const [contextMenu, setContextMenu] = useState<ContextMenuState>({
    isOpen: false,
    x: 0,
    y: 0,
    task: null,
  });

  useEffect(() => {
    setIsMounted(true);
  }, []);

  /** localStorage에서 저장된 필터를 복원한다 */
  useEffect(() => {
    const stored = localStorage.getItem(FILTER_STORAGE_KEY);
    if (!stored) return;

    try {
      const parsed = JSON.parse(stored);
      if (Array.isArray(parsed)) {
        const validIds = parsed.filter((id: string) =>
          projects.some((p) => p.id === id)
        );
        if (validIds.length > 0) setSelectedProjectIds(validIds);
      }
    } catch {
      /** 이전 단일 선택 포맷 호환: plain string */
      if (projects.some((p) => p.id === stored)) {
        setSelectedProjectIds([stored]);
      }
    }
  }, [projects]);

  /** 필터 변경 시 localStorage에 저장한다 */
  useEffect(() => {
    if (selectedProjectIds.length > 0) {
      localStorage.setItem(FILTER_STORAGE_KEY, JSON.stringify(selectedProjectIds));
    } else {
      localStorage.removeItem(FILTER_STORAGE_KEY);
    }
  }, [selectedProjectIds]);

  /** 서버 revalidation 후 initialTasks가 변경되면 로컬 state에 반영한다 */
  useEffect(() => {
    setTasks(initialTasks);
    setDoneTotal(initialDoneTotal);
    setDoneOffset(initialDoneLimit);
  }, [initialTasks, initialDoneTotal, initialDoneLimit]);

  const handleLoadMoreDone = useCallback(async () => {
    if (isLoadingMore) return;
    setIsLoadingMore(true);
    try {
      const result = await getMoreDoneTasks(doneOffset);
      setTasks((prev) => ({
        ...prev,
        [TaskStatus.DONE]: [...prev[TaskStatus.DONE], ...result.tasks],
      }));
      setDoneOffset((prev) => prev + result.tasks.length);
      setDoneTotal(result.doneTotal);
    } finally {
      setIsLoadingMore(false);
    }
  }, [doneOffset, isLoadingMore]);

  const handleDragEnd = useCallback(
    (result: DropResult) => {
      const { source, destination, draggableId } = result;
      if (!destination) return;

      const sourceStatus = source.droppableId as TaskStatus;
      const destStatus = destination.droppableId as TaskStatus;

      setTasks((prev) => {
        const updated = { ...prev };

        const taskIndex = updated[sourceStatus].findIndex(
          (task) => task.id === draggableId
        );
        if (taskIndex === -1) return prev;

        const movedTask = updated[sourceStatus][taskIndex];
        const newSource = updated[sourceStatus].filter(
          (task) => task.id !== draggableId
        );

        if (sourceStatus === destStatus) {
          updated[sourceStatus] = insertAtFilteredIndex(
            newSource,
            movedTask,
            destination.index,
            projectFilterSet
          );

          const reorderIds = (
            projectFilterSet
              ? updated[sourceStatus].filter(
                  (task) =>
                    task.projectId && projectFilterSet.has(task.projectId)
                )
              : updated[sourceStatus]
          ).map((task) => task.id);

          reorderTasks(sourceStatus, reorderIds);
        } else {
          updated[sourceStatus] = newSource;
          const updatedTask: KanbanTask = { ...movedTask, status: destStatus };
          updated[destStatus] = insertAtFilteredIndex(
            updated[destStatus],
            updatedTask,
            destination.index,
            projectFilterSet
          );

          if (destStatus === TaskStatus.DONE) {
            setDoneTotal((prev) => prev + 1);
            setDoneOffset((prev) => prev + 1);
          }
          if (sourceStatus === TaskStatus.DONE) {
            setDoneTotal((prev) => prev - 1);
            setDoneOffset((prev) => prev - 1);
          }

          /** 상태 변경 + 목적지 컬럼 순서를 한 번에 DB에 반영한다 (revalidation 없음) */
          const destReorderIds = (
            projectFilterSet
              ? updated[destStatus].filter(
                  (task) =>
                    task.projectId && projectFilterSet.has(task.projectId)
                )
              : updated[destStatus]
          ).map((task) => task.id);

          moveTaskToColumn(draggableId, destStatus, destReorderIds);
        }

        return updated;
      });
    },
    [projectFilterSet]
  );

  const handleContextMenu = useCallback(
    (e: React.MouseEvent, task: KanbanTask) => {
      e.preventDefault();
      setContextMenu({ isOpen: true, x: e.clientX, y: e.clientY, task });
    },
    []
  );

  const handleCloseContextMenu = useCallback(() => {
    setContextMenu((prev) => ({ ...prev, isOpen: false, task: null }));
  }, []);

  const handleBranchFromCard = useCallback(() => {
    setIsBranchModalOpen(true);
    setContextMenu((prev) => ({ ...prev, isOpen: false }));
  }, []);

  const handleDeleteFromCard = useCallback(() => {
    if (contextMenu.task && confirm(tt("deleteConfirm"))) {
      deleteTask(contextMenu.task.id);
    }
    handleCloseContextMenu();
  }, [contextMenu.task, handleCloseContextMenu, tt]);

  return (
    <div className="min-h-screen">
      <header className="flex items-center justify-between px-6 py-4 border-b border-border-default bg-bg-surface">
        <h1 className="text-xl font-bold text-text-primary">{t("title")}</h1>
        <div className="flex items-center gap-3">
          <div className="w-64">
            <ProjectSelector
              multiple
              projects={filterableProjects}
              selectedProjectIds={selectedProjectIds}
              onSelectionChange={setSelectedProjectIds}
              placeholder={t("allProjects")}
              searchPlaceholder={tt("projectSearch")}
              compact
            />
          </div>
          <button
            onClick={() => setIsModalOpen(true)}
            className="px-4 py-1.5 text-sm bg-brand-primary hover:bg-brand-hover text-text-inverse rounded-md font-medium transition-colors"
          >
            {t("newTask")}
          </button>
          <button
            onClick={() => setIsSettingsOpen(true)}
            className="px-3 py-1.5 text-sm text-text-muted hover:text-text-primary transition-colors"
            title={tc("settings")}
          >
            &#9881;
          </button>
          <form action={logoutAction}>
            <button
              type="submit"
              className="px-3 py-1.5 text-sm text-text-muted hover:text-text-primary transition-colors"
            >
              {tc("logout")}
            </button>
          </form>
        </div>
      </header>

      <main className="p-6">
        {isMounted ? (
          <DragDropContext onDragEnd={handleDragEnd}>
            <div className="flex gap-4 overflow-x-auto">
              {COLUMNS.map((col) => (
                <Column
                  key={col.status}
                  status={col.status}
                  tasks={filteredTasks[col.status]}
                  label={t(`columns.${col.labelKey}`)}
                  colorClass={col.colorClass}
                  onContextMenu={handleContextMenu}
                  projectNameMap={projectNameMap}
                  {...(col.status === TaskStatus.DONE && {
                    totalCount: doneTotal,
                    hasMore: doneOffset < doneTotal,
                    onLoadMore: handleLoadMoreDone,
                    isLoadingMore,
                  })}
                />
              ))}
            </div>
          </DragDropContext>
        ) : (
          <div className="flex gap-4 overflow-x-auto">
            {COLUMNS.map((col) => (
              <div key={col.status} className="flex-1 min-w-[280px] max-w-[350px]">
                <div className="flex items-center gap-2 mb-3 px-2">
                  <div className={`w-3 h-3 rounded-full ${col.colorClass}`} />
                  <h2 className="text-sm font-semibold text-text-secondary uppercase tracking-wide">
                    {t(`columns.${col.labelKey}`)}
                  </h2>
                </div>
                <div className="min-h-[200px] p-2 rounded-lg" />
              </div>
            ))}
          </div>
        )}
      </main>

      <CreateTaskModal
        isOpen={isModalOpen}
        onClose={() => setIsModalOpen(false)}
        sshHosts={sshHosts}
        projects={projects}
        defaultProjectId={selectedProjectIds.length === 1 ? selectedProjectIds[0] : ""}
      />

      <ProjectSettings
        isOpen={isSettingsOpen}
        onClose={() => setIsSettingsOpen(false)}
        projects={projects}
        sshHosts={sshHosts}
        sidebarDefaultCollapsed={sidebarDefaultCollapsed}
      />

      {contextMenu.isOpen && contextMenu.task && (
        <TaskContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          onClose={handleCloseContextMenu}
          onBranch={handleBranchFromCard}
          onDelete={handleDeleteFromCard}
          hasBranch={!!contextMenu.task.branchName}
        />
      )}

      {isBranchModalOpen && contextMenu.task && (
        <BranchTaskModal
          task={contextMenu.task}
          projects={projects}
          onClose={() => {
            setIsBranchModalOpen(false);
            handleCloseContextMenu();
          }}
        />
      )}
    </div>
  );
}
