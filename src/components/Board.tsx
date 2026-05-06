"use client";

import { useState, useCallback, useEffect, useMemo, useRef, useTransition } from "react";
import { useTranslations } from "next-intl";
import { DragDropContext, type DropResult } from "@hello-pangea/dnd";
import BoardPageFindBar from "./BoardPageFindBar";
import Column from "./Column";
import CreateTaskModal from "./CreateTaskModal";
import NotificationCenterButton from "./NotificationCenterButton";
import ProjectSelector from "./ProjectSelector";
import TaskContextMenu from "./TaskContextMenu";
import BranchTaskModal from "./BranchTaskModal";
import DoneConfirmDialog from "./DoneConfirmDialog";
import { reorderTasks, deleteTask, getMoreDoneTasks, moveTaskToColumn } from "@/desktop/renderer/actions/kanban";
import type { TasksByStatus } from "@/desktop/renderer/actions/kanban";
import { useBoardCommands } from "@/desktop/renderer/components/BoardCommandProvider";
import { localizeHref } from "@/desktop/renderer/navigation";
import { openInternalRouteInNewWindow } from "@/desktop/renderer/utils/windowOpen";
import { SessionType, TaskStatus, type KanbanTask } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";
import { useAutoRefresh } from "@/desktop/renderer/hooks/useAutoRefresh";
import { useProjectFilterParams } from "@/desktop/renderer/hooks/useProjectFilterParams";
import { DEFAULT_LOCALE, SUPPORTED_LOCALES } from "@/desktop/renderer/utils/locales";
import { computeProjectColor } from "@/lib/projectColor";
import type { NotificationCenterButtonHandle } from "./NotificationCenterButton";
import type { ProjectSelectorHandle } from "./ProjectSelector";

interface BoardProps {
  initialTasks: TasksByStatus;
  initialDoneTotal: number;
  initialDoneLimit: number;
  sshHosts: string[];
  projects: Project[];
  sidebarDefaultCollapsed: boolean;
  doneAlertDismissed: boolean;
  notificationSettings: { isEnabled: boolean; enabledStatuses: string[] };
  defaultSessionType: SessionType;
  taskSearchShortcut: string;
}

const COLUMNS: { status: TaskStatus; labelKey: string; colorClass: string }[] = [
  { status: TaskStatus.TODO, labelKey: "todo", colorClass: "bg-status-todo" },
  { status: TaskStatus.PROGRESS, labelKey: "progress", colorClass: "bg-status-progress" },
  { status: TaskStatus.PENDING, labelKey: "pending", colorClass: "bg-status-pending" },
  { status: TaskStatus.REVIEW, labelKey: "review", colorClass: "bg-status-review" },
  { status: TaskStatus.DONE, labelKey: "done", colorClass: "bg-status-done" },
];

const TASK_CARD_SELECTOR = "[data-kanban-task-card='true']";
const BOARD_TASK_FOCUS_KEYS = new Set(["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"]);

interface ContextMenuState {
  isOpen: boolean;
  x: number;
  y: number;
  task: KanbanTask | null;
}

function getBoardTaskCards() {
  return Array.from(document.querySelectorAll<HTMLAnchorElement>(TASK_CARD_SELECTOR));
}

function focusBoardTaskCard(card: HTMLAnchorElement) {
  card.focus({ preventScroll: true });
  card.scrollIntoView?.({ block: "nearest", inline: "nearest" });
}

function findTaskById(tasks: TasksByStatus, taskId: string) {
  for (const status of Object.values(TaskStatus)) {
    const task = tasks[status].find((candidate) => candidate.id === taskId);
    if (task) return task;
  }

  return null;
}

function shouldIgnoreBoardTaskFocusEvent(event: KeyboardEvent) {
  if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey) {
    return true;
  }

  const target = event.target instanceof Element
    ? event.target
    : document.activeElement instanceof Element
      ? document.activeElement
      : null;

  if (!target) return false;

  return Boolean(
    target.closest(
      [
            TASK_CARD_SELECTOR,
            "a[href]",
            "[role='link']",
            "input",
            "textarea",
            "select",
        "button",
        "[contenteditable='true']",
        "[data-terminal-focus-blocker='true']",
        "[role='menu']",
        "[role='menuitem']",
        "[role='dialog']",
      ].join(","),
    ),
  );
}

function getTaskCardFromKeyboardEvent(event: KeyboardEvent) {
  const target = event.target instanceof Element
    ? event.target
    : document.activeElement instanceof Element
      ? document.activeElement
      : null;

  return target?.closest<HTMLAnchorElement>(TASK_CARD_SELECTOR) ?? null;
}

function isShiftOnlyKeyboardShortcut(event: KeyboardEvent, key: string) {
  return event.key === key && event.shiftKey && !event.altKey && !event.ctrlKey && !event.metaKey;
}

function getCurrentBoardLocale() {
  const firstSegment = window.location.hash.replace(/^#/, "").split("/").filter(Boolean)[0];
  return SUPPORTED_LOCALES.includes(firstSegment as typeof SUPPORTED_LOCALES[number])
    ? firstSegment
    : DEFAULT_LOCALE;
}

function openTaskDetailInNewWindow(taskId: string) {
  openInternalRouteInNewWindow(localizeHref(`/task/${taskId}`, getCurrentBoardLocale()));
}

function buildStatusMoveResult(
  task: KanbanTask,
  destinationStatus: TaskStatus,
  currentTasks: TasksByStatus,
  filteredTasks: TasksByStatus,
): DropResult | null {
  if (task.status === destinationStatus) return null;

  const sourceIndex = currentTasks[task.status].findIndex((candidate) => candidate.id === task.id);
  if (sourceIndex === -1) return null;

  return {
    draggableId: task.id,
    type: "DEFAULT",
    source: {
      droppableId: task.status,
      index: sourceIndex,
    },
    destination: {
      droppableId: destinationStatus,
      index: filteredTasks[destinationStatus].length,
    },
    reason: "DROP",
    mode: "FLUID",
    combine: null,
  };
}

/** worktree repoPath에서 메인 프로젝트 경로를 추출한다 */
function extractMainRepoPath(repoPath: string): string | null {
  const worktreeIndex = repoPath.indexOf("__worktrees");
  if (worktreeIndex === -1) return null;
  return repoPath.slice(0, worktreeIndex);
}

function openSettingsPage() {
  window.location.hash = `#/${getCurrentBoardLocale()}/settings`;
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

interface DragMovePlan {
  updatedTasks: TasksByStatus;
  doneTotalDelta: number;
  doneOffsetDelta: number;
  persistence:
    | { type: "reorder"; status: TaskStatus; orderedIds: string[] }
    | { type: "move"; taskId: string; status: TaskStatus; orderedIds: string[] };
}

function buildDragMovePlan(
  currentTasks: TasksByStatus,
  result: DropResult,
  projectFilterSet: Set<string> | null,
): DragMovePlan | null {
  const { source, destination, draggableId } = result;
  if (!destination) return null;

  const sourceStatus = source.droppableId as TaskStatus;
  const destStatus = destination.droppableId as TaskStatus;
  const updated: TasksByStatus = { ...currentTasks };

  const taskIndex = updated[sourceStatus].findIndex((task) => task.id === draggableId);
  if (taskIndex === -1) return null;

  const movedTask = updated[sourceStatus][taskIndex];
  const newSource = updated[sourceStatus].filter((task) => task.id !== draggableId);

  if (sourceStatus === destStatus) {
    updated[sourceStatus] = insertAtFilteredIndex(
      newSource,
      movedTask,
      destination.index,
      projectFilterSet,
    );

    const orderedIds = (
      projectFilterSet
        ? updated[sourceStatus].filter(
            (task) => task.projectId && projectFilterSet.has(task.projectId),
          )
        : updated[sourceStatus]
    ).map((task) => task.id);

    return {
      updatedTasks: updated,
      doneTotalDelta: 0,
      doneOffsetDelta: 0,
      persistence: {
        type: "reorder",
        status: sourceStatus,
        orderedIds,
      },
    };
  }

  updated[sourceStatus] = newSource;
  const updatedTask: KanbanTask = { ...movedTask, status: destStatus };
  updated[destStatus] = insertAtFilteredIndex(
    updated[destStatus],
    updatedTask,
    destination.index,
    projectFilterSet,
  );

  const orderedIds = (
    projectFilterSet
      ? updated[destStatus].filter(
          (task) => task.projectId && projectFilterSet.has(task.projectId),
        )
      : updated[destStatus]
  ).map((task) => task.id);

  return {
    updatedTasks: updated,
    doneTotalDelta:
      (destStatus === TaskStatus.DONE ? 1 : 0) -
      (sourceStatus === TaskStatus.DONE ? 1 : 0),
    doneOffsetDelta:
      (destStatus === TaskStatus.DONE ? 1 : 0) -
      (sourceStatus === TaskStatus.DONE ? 1 : 0),
    persistence: {
      type: "move",
      taskId: draggableId,
      status: destStatus,
      orderedIds,
    },
  };
}

export default function Board({
  initialTasks,
  initialDoneTotal,
  initialDoneLimit,
  sshHosts,
  projects,
  doneAlertDismissed,
  defaultSessionType,
}: BoardProps) {
  useAutoRefresh();
  const boardCommands = useBoardCommands();
  const t = useTranslations("board");
  const tt = useTranslations("task");
  const tc = useTranslations("common");
  const [tasks, setTasks] = useState<TasksByStatus>(initialTasks);
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [isBranchModalOpen, setIsBranchModalOpen] = useState(false);
  const [branchTodoDefaults, setBranchTodoDefaults] = useState<{
    baseBranch: string;
    projectId: string;
  } | null>(null);
  const [isMounted, setIsMounted] = useState(false);
  const [selectedProjectIds, setSelectedProjectIds] = useProjectFilterParams(
    projects.map((p) => p.id),
  );
  const [doneTotal, setDoneTotal] = useState(initialDoneTotal);
  const [doneOffset, setDoneOffset] = useState(initialDoneLimit);
  const [isLoadingMore, setIsLoadingMore] = useState(false);
  const [isDoneAlertDismissed, setIsDoneAlertDismissed] = useState(doneAlertDismissed);
  const [pendingDoneResult, setPendingDoneResult] = useState<DropResult | null>(null);
  const [currentDefaultSessionType, setCurrentDefaultSessionType] = useState<SessionType>(defaultSessionType);
  const [shouldUseMacTitlebarLayout, setShouldUseMacTitlebarLayout] = useState(false);
  const [, startDragPersistenceTransition] = useTransition();
  const notificationCenterRef = useRef<NotificationCenterButtonHandle>(null);
  const projectSelectorRef = useRef<ProjectSelectorHandle>(null);

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

  /** 프로젝트명 → hex 색상 매핑. DB color 우선, 없으면 해시 기반 프리셋 할당 */
  const projectColorMap = useMemo(() => {
    const colorMap: Record<string, string> = {};

    const uniqueNames = new Set(Object.values(projectNameMap));
    for (const name of uniqueNames) {
      const mainProject = projects.find(
        (p) => p.name === name && !extractMainRepoPath(p.repoPath)
      );
      if (mainProject?.color) {
        colorMap[name] = mainProject.color;
      } else {
        colorMap[name] = computeProjectColor(name);
      }
    }
    return colorMap;
  }, [projectNameMap, projects]);

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
      [TaskStatus.PENDING]: [],
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

  /** 서버 revalidation 후 initialTasks가 변경되면 로컬 state에 반영한다 */
  useEffect(() => {
    setTasks(initialTasks);
    setDoneTotal(initialDoneTotal);
    setDoneOffset(initialDoneLimit);
  }, [initialTasks, initialDoneTotal, initialDoneLimit]);

  useEffect(() => {
    setCurrentDefaultSessionType(defaultSessionType);
  }, [defaultSessionType]);

  useEffect(() => {
    const isDesktopApp = window.kanvibeDesktop?.isDesktop === true;
    const isMacDesktop = navigator.userAgent.includes("Mac") || navigator.platform.toLowerCase().includes("mac");
    setShouldUseMacTitlebarLayout(isDesktopApp && isMacDesktop);
  }, []);

  useEffect(() => boardCommands.registerBoardHandlers({
    toggleNotificationCenter() {
      notificationCenterRef.current?.toggle();
    },
    openProjectFilter() {
      projectSelectorRef.current?.open();
    },
    openCreateTaskModal(defaults) {
      setBranchTodoDefaults(defaults ?? null);
      setIsModalOpen(true);
    },
  }), [boardCommands]);

  useEffect(() => {
    function handleWindowTaskFocus(event: KeyboardEvent) {
      if (!BOARD_TASK_FOCUS_KEYS.has(event.key)) return;
      if (contextMenu.isOpen || isModalOpen || isBranchModalOpen || pendingDoneResult) return;
      if (shouldIgnoreBoardTaskFocusEvent(event)) return;

      const firstTaskCard = getBoardTaskCards()[0];
      if (!firstTaskCard) return;

      event.preventDefault();
      focusBoardTaskCard(firstTaskCard);
    }

    window.addEventListener("keydown", handleWindowTaskFocus);
    return () => window.removeEventListener("keydown", handleWindowTaskFocus);
  }, [contextMenu.isOpen, isBranchModalOpen, isModalOpen, pendingDoneResult]);

  useEffect(() => {
    function handleWindowTaskShortcut(event: KeyboardEvent) {
      const shouldOpenTaskInNewWindow = isShiftOnlyKeyboardShortcut(event, "Enter");
      const shouldOpenTaskContextMenu = isShiftOnlyKeyboardShortcut(event, "F10");
      if (!shouldOpenTaskInNewWindow && !shouldOpenTaskContextMenu) return;
      if (contextMenu.isOpen || isModalOpen || isBranchModalOpen || pendingDoneResult) return;

      const taskCard = getTaskCardFromKeyboardEvent(event);
      const taskId = taskCard?.dataset.kanbanTaskId;
      if (!taskCard || !taskId) return;

      event.preventDefault();
      event.stopPropagation();

      if (shouldOpenTaskInNewWindow) {
        openTaskDetailInNewWindow(taskId);
        return;
      }

      const task = findTaskById(filteredTasks, taskId);
      if (!task) return;

      const rect = taskCard.getBoundingClientRect();
      setContextMenu({ isOpen: true, x: rect.left + 12, y: rect.top + 12, task });
    }

    window.addEventListener("keydown", handleWindowTaskShortcut, true);
    return () => window.removeEventListener("keydown", handleWindowTaskShortcut, true);
  }, [contextMenu.isOpen, filteredTasks, isBranchModalOpen, isModalOpen, pendingDoneResult]);

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

  /** 드래그 결과를 받아 state 업데이트 + DB 반영을 수행한다 */
  const executeDragMove = useCallback(
    (result: DropResult) => {
      const plan = buildDragMovePlan(tasks, result, projectFilterSet);
      if (!plan) return;

      setTasks(plan.updatedTasks);

      if (plan.doneTotalDelta !== 0) {
        setDoneTotal((prev) => prev + plan.doneTotalDelta);
      }

      if (plan.doneOffsetDelta !== 0) {
        setDoneOffset((prev) => prev + plan.doneOffsetDelta);
      }

      startDragPersistenceTransition(async () => {
        if (plan.persistence.type === "reorder") {
          await reorderTasks(plan.persistence.status, plan.persistence.orderedIds);
          return;
        }

        await moveTaskToColumn(
          plan.persistence.taskId,
          plan.persistence.status,
          plan.persistence.orderedIds,
        );
      });
    },
    [projectFilterSet, startDragPersistenceTransition, tasks]
  );

  const handleDragEnd = useCallback(
    (result: DropResult) => {
      const { source, destination, draggableId } = result;
      if (!destination) return;

      const sourceStatus = source.droppableId as TaskStatus;
      const destStatus = destination.droppableId as TaskStatus;

      /** Done 이동 시 리소스 삭제 경고 (dismissed 아닌 경우만) */
      if (destStatus === TaskStatus.DONE && sourceStatus !== destStatus && !isDoneAlertDismissed) {
        const task = tasks[sourceStatus].find((task) => task.id === draggableId);
        const hasCleanableResources = task && (task.branchName || task.sessionType);
        if (hasCleanableResources) {
          setPendingDoneResult(result);
          return;
        }
      }

      executeDragMove(result);
    },
    [tasks, isDoneAlertDismissed, executeDragMove]
  );

  const handleDoneConfirm = useCallback(() => {
    if (pendingDoneResult) {
      executeDragMove(pendingDoneResult);
      setIsDoneAlertDismissed(true);
    }
    setPendingDoneResult(null);
  }, [pendingDoneResult, executeDragMove]);

  const handleDoneCancel = useCallback(() => {
    setPendingDoneResult(null);
  }, []);

  const handleContextMenu = useCallback(
    (task: KanbanTask, position: { x: number; y: number }) => {
      setContextMenu({ isOpen: true, x: position.x, y: position.y, task });
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

  /** 우클릭한 태스크의 브랜치를 base로 새 TODO를 생성하는 모달을 연다 */
  const handleCreateBranchTodo = useCallback(() => {
    if (contextMenu.task?.branchName && contextMenu.task?.projectId) {
      setBranchTodoDefaults({
        baseBranch: contextMenu.task.branchName,
        projectId: contextMenu.task.projectId,
      });
      setIsModalOpen(true);
    }
    setContextMenu((prev) => ({ ...prev, isOpen: false }));
  }, [contextMenu.task]);

  const handleDeleteFromCard = useCallback(() => {
    if (contextMenu.task && confirm(tt("deleteConfirm"))) {
      deleteTask(contextMenu.task.id);
    }
    handleCloseContextMenu();
  }, [contextMenu.task, handleCloseContextMenu, tt]);

  const handleStatusChangeFromCard = useCallback(
    (newStatus: TaskStatus) => {
      const task = contextMenu.task;
      if (!task) return;

      const result = buildStatusMoveResult(task, newStatus, tasks, filteredTasks);
      handleCloseContextMenu();

      if (!result) return;

      const shouldConfirmDoneMove =
        newStatus === TaskStatus.DONE &&
        task.status !== TaskStatus.DONE &&
        !isDoneAlertDismissed &&
        !!(task.branchName || task.sessionType);

      if (shouldConfirmDoneMove) {
        setPendingDoneResult(result);
        return;
      }

      executeDragMove(result);
    },
    [
      contextMenu.task,
      executeDragMove,
      filteredTasks,
      handleCloseContextMenu,
      isDoneAlertDismissed,
      tasks,
    ],
  );

  const headerClassName = shouldUseMacTitlebarLayout
    ? "flex items-center justify-end bg-bg-page px-6 pb-3 pl-20 pr-6 pt-10 [-webkit-app-region:drag]"
    : "flex items-center justify-end border-b border-border-default bg-bg-surface px-6 py-3";

  const mainClassName = shouldUseMacTitlebarLayout ? "px-6 pb-6" : "p-6";

  return (
    <div className="min-h-screen bg-bg-page">
      <BoardPageFindBar />
      <header className={headerClassName}>
        <div className="flex items-center gap-3 [-webkit-app-region:no-drag]">
          <div className="w-64">
            <ProjectSelector
              ref={projectSelectorRef}
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
            onClick={openSettingsPage}
            className="p-1.5 rounded-md border border-transparent text-text-muted transition-colors hover:border-border-default hover:bg-bg-page hover:text-text-primary"
            title={tc("settings")}
            aria-label={tc("settings")}
          >
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/>
              <circle cx="12" cy="12" r="3"/>
            </svg>
          </button>
          <NotificationCenterButton ref={notificationCenterRef} buttonClassName="hover:bg-bg-page" />
        </div>
      </header>

      <main className={mainClassName}>
        {isMounted ? (
          <DragDropContext onDragEnd={handleDragEnd}>
            <div className="flex gap-3 overflow-x-auto pb-2">
              {COLUMNS.map((col) => (
                <Column
                  key={col.status}
                  status={col.status}
                  tasks={filteredTasks[col.status]}
                  label={t(`columns.${col.labelKey}`)}
                  colorClass={col.colorClass}
                  onContextMenu={handleContextMenu}
                  projectNameMap={projectNameMap}
                  projectColorMap={projectColorMap}
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
        onClose={() => {
          setIsModalOpen(false);
          setBranchTodoDefaults(null);
        }}
        sshHosts={sshHosts}
        projects={projects}
        defaultProjectId={branchTodoDefaults?.projectId || (selectedProjectIds.length === 1 ? selectedProjectIds[0] : "")}
        defaultBaseBranch={branchTodoDefaults?.baseBranch}
        defaultSessionType={currentDefaultSessionType}
      />

      {contextMenu.isOpen && contextMenu.task && (
        <TaskContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          onClose={handleCloseContextMenu}
          onBranch={handleBranchFromCard}
          onCreateBranchTodo={handleCreateBranchTodo}
          onStatusChange={handleStatusChangeFromCard}
          onDelete={handleDeleteFromCard}
          hasBranch={!!contextMenu.task.branchName}
          currentStatus={contextMenu.task.status}
          statusOptions={COLUMNS.map((column) => ({
            status: column.status,
            label: t(`columns.${column.labelKey}`),
            colorClass: column.colorClass,
          }))}
        />
      )}

      {isBranchModalOpen && contextMenu.task && (
        <BranchTaskModal
          task={contextMenu.task}
          projects={projects}
          defaultSessionType={currentDefaultSessionType}
          onClose={() => {
            setIsBranchModalOpen(false);
            handleCloseContextMenu();
          }}
        />
      )}

      <DoneConfirmDialog
        isOpen={!!pendingDoneResult}
        onConfirm={handleDoneConfirm}
        onCancel={handleDoneCancel}
      />
    </div>
  );
}
