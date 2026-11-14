"use client";

import { useState, useCallback, useEffect, useMemo, useRef } from "react";
import { useLocale, useTranslations } from "next-intl";
import { DragDropContext, type DropResult } from "@hello-pangea/dnd";
import BoardPageFindBar from "./BoardPageFindBar";
import BoardSortPicker from "./BoardSortPicker";
import DesktopFilter from "./DesktopFilter";
import Column from "./Column";
import type { TaskDeletionState, TaskMoveState } from "./TaskCard";
import CreateTaskModal from "./CreateTaskModal";
import ProjectRegistryDialog from "./ProjectRegistryDialog";
import NotificationCenterButton from "./NotificationCenterButton";
import ProjectSelector from "./ProjectSelector";
import TaskContextMenu from "./TaskContextMenu";
import BranchTaskModal from "./BranchTaskModal";
import DoneConfirmDialog from "./DoneConfirmDialog";
import { deleteTask, getMoreDoneTasks, moveTaskToColumn } from "@/desktop/renderer/actions/kanban";
import type { TasksByStatus } from "@/desktop/renderer/actions/kanban";
import { useBoardCommands } from "@/desktop/renderer/components/BoardCommandProvider";
import { navigateToTaskDetail } from "@/desktop/renderer/utils/taskNavigation";
import { getFocusedBoardTaskCard } from "@/desktop/renderer/utils/focusedBoardTaskCard";
import { SessionType, TaskStatus, type KanbanTask } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";
import { useAutoRefresh } from "@/desktop/renderer/hooks/useAutoRefresh";
import { useProjectFilterParams } from "@/desktop/renderer/hooks/useProjectFilterParams";
import { useRunningAgentPanes } from "@/desktop/renderer/hooks/useLiveAiSessions";
import { useBoardTaskDiffStats } from "@/desktop/renderer/hooks/useTaskDiffStats";
import { useUnreadNotificationCountByTask } from "@/desktop/renderer/hooks/useUnreadNotificationCountByTask";
import {
  TASK_KIND_FILTER_VALUES,
  useTaskKindFilterParams,
} from "@/desktop/renderer/hooks/useTaskKindFilterParams";
import { useBoardSortPreference } from "@/desktop/renderer/hooks/useBoardSortPreference";
import {
  buildProjectFilterSet,
  buildProjectRootPriorityMap,
  matchesProjectFilter,
  matchesTaskKindFilter,
  sortTasksForBoard,
} from "@/desktop/renderer/utils/boardTaskSort";
import { DEFAULT_LOCALE, SUPPORTED_LOCALES } from "@/desktop/renderer/utils/locales";
import { buildProjectDisplayMap } from "@/desktop/renderer/utils/projectDisplay";
import { computeProjectColor } from "@/lib/projectColor";
import type { NotificationCenterButtonHandle } from "./NotificationCenterButton";
import type { ProjectSelectorHandle } from "./ProjectSelector";

interface BoardProps {
  initialTasks: TasksByStatus;
  initialDoneTotal: number;
  initialDoneLimit: number;
  initialFocusTaskId?: string | null;
  sshHosts: string[];
  projects: Project[];
  sidebarDefaultCollapsed: boolean;
  doneAlertDismissed: boolean;
  notificationSettings: { isEnabled: boolean; enabledStatuses: string[] };
  defaultSessionType: SessionType;
  taskSearchShortcut: string;
  vimModeEnabled?: boolean;
}

const COLUMNS: { status: TaskStatus; labelKey: string; colorClass: string }[] = [
  { status: TaskStatus.TODO, labelKey: "todo", colorClass: "bg-status-todo" },
  { status: TaskStatus.PROGRESS, labelKey: "progress", colorClass: "bg-status-progress" },
  { status: TaskStatus.PENDING, labelKey: "pending", colorClass: "bg-status-pending" },
  { status: TaskStatus.REVIEW, labelKey: "review", colorClass: "bg-status-review" },
  { status: TaskStatus.DONE, labelKey: "done", colorClass: "bg-status-done" },
];

const TASK_CARD_SELECTOR = "[data-kanban-task-card='true']";
const BOARD_ARROW_TASK_FOCUS_KEYS = new Set([
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
]);
const BOARD_VIM_TASK_FOCUS_KEYS = new Set([
  "h",
  "j",
  "k",
  "l",
]);
const VIM_NEW_TASK_KEY = "n";
const VIM_COMMAND_KEY = ":";
const TASK_DELETE_SEQUENCE_KEY = "d";
const TASK_DELETE_SEQUENCE_TIMEOUT_MS = 1_000;

type BoardTaskFilter = (task: KanbanTask) => boolean;

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

function findBoardTaskCardById(taskId: string) {
  return getBoardTaskCards().find((card) => card.dataset.kanbanTaskId === taskId) ?? null;
}

function findTaskById(tasks: TasksByStatus, taskId: string) {
  for (const status of Object.values(TaskStatus)) {
    const task = tasks[status].find((candidate) => candidate.id === taskId);
    if (task) return task;
  }

  return null;
}

function removeTaskFromBoardTasks(currentTasks: TasksByStatus, taskId: string) {
  let didRemoveTask = false;
  const nextTasks: TasksByStatus = { ...currentTasks };

  for (const status of Object.values(TaskStatus)) {
    const remainingTasks = currentTasks[status].filter((candidate) => candidate.id !== taskId);
    if (remainingTasks.length !== currentTasks[status].length) {
      nextTasks[status] = remainingTasks;
      didRemoveTask = true;
    }
  }

  return didRemoveTask ? nextTasks : currentTasks;
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

function shouldIgnoreBoardVimShortcutEvent(event: KeyboardEvent) {
  if (event.defaultPrevented) {
    return true;
  }

  const target = event.target instanceof Element
    ? event.target
    : document.activeElement instanceof Element
      ? document.activeElement
      : null;

  if (!target) return false;
  if (target.closest(TASK_CARD_SELECTOR)) return false;

  return Boolean(
    target.closest(
      [
        "input",
        "textarea",
        "select",
        "button",
        "[contenteditable='true']",
        "[data-shortcut-capture='true']",
        "[data-terminal-focus-blocker='true']",
        "[role='dialog']",
        "[role='menu']",
        "[role='menuitem']",
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

function isPlainTaskDeleteSequenceKey(event: KeyboardEvent) {
  return event.key === TASK_DELETE_SEQUENCE_KEY && !event.shiftKey && !event.altKey && !event.ctrlKey && !event.metaKey;
}

function isBoardTaskFocusKey(event: KeyboardEvent, vimModeEnabled: boolean) {
  if (BOARD_ARROW_TASK_FOCUS_KEYS.has(event.key)) return true;
  return vimModeEnabled && BOARD_VIM_TASK_FOCUS_KEYS.has(event.key);
}

function isPlainVimShortcutKey(event: KeyboardEvent, key: string) {
  return event.key === key && !event.shiftKey && !event.altKey && !event.ctrlKey && !event.metaKey;
}

function isVimCommandShortcutKey(event: KeyboardEvent) {
  return event.key === VIM_COMMAND_KEY && !event.altKey && !event.ctrlKey && !event.metaKey;
}

function isModifierKey(key: string) {
  return key === "Alt" || key === "Control" || key === "Meta" || key === "Shift";
}

function getCurrentBoardLocale() {
  const firstSegment = window.location.hash.replace(/^#/, "").split("/").filter(Boolean)[0];
  return SUPPORTED_LOCALES.includes(firstSegment as typeof SUPPORTED_LOCALES[number])
    ? firstSegment
    : DEFAULT_LOCALE;
}

function openTaskDetailInNewWindow(taskId: string) {
  void navigateToTaskDetail(taskId, {
    currentLocale: getCurrentBoardLocale(),
    openInNewWindow: true,
  });
}

function buildStatusMoveResult(
  task: KanbanTask,
  destinationStatus: TaskStatus,
  currentTasks: TasksByStatus,
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
      index: 0,
    },
    reason: "DROP",
    mode: "FLUID",
    combine: null,
  };
}

function openSettingsPage() {
  window.location.hash = `#/${getCurrentBoardLocale()}/settings`;
}

interface DragMovePlan {
  updatedTasks: TasksByStatus;
  doneTotalDelta: number;
  doneOffsetDelta: number;
  persistence: { taskId: string; status: TaskStatus };
  /** 저장에 실패하면 이 task만 원래 칸·자리로 되돌린다 */
  restore: { task: KanbanTask; status: TaskStatus; index: number };
}

/** 저장에 실패한 task 하나만 원래 자리로 돌린다. 그 사이 다른 카드에 생긴 변화는 덮지 않는다 */
function restoreMovedTask(currentTasks: TasksByStatus, plan: DragMovePlan): TasksByStatus {
  const { taskId, status: movedStatus } = plan.persistence;
  const { task, status: originalStatus, index } = plan.restore;
  if (!currentTasks[movedStatus].some((candidate) => candidate.id === taskId)) {
    return currentTasks;
  }

  const originalColumn = [...currentTasks[originalStatus]];
  originalColumn.splice(Math.min(index, originalColumn.length), 0, task);
  return {
    ...currentTasks,
    [movedStatus]: currentTasks[movedStatus].filter((candidate) => candidate.id !== taskId),
    [originalStatus]: originalColumn,
  };
}

/** task별 요청 상태 기록에서 한 항목을 바꾸거나(null이면) 지운다 */
function setRequestStateEntry<T>(states: Record<string, T>, taskId: string, state: T | null): Record<string, T> {
  const next = { ...states };
  if (state) {
    next[taskId] = state;
  } else {
    delete next[taskId];
  }
  return next;
}

/**
 * 드래그 결과를 낙관적 상태 변경과 저장 계획으로 바꾼다.
 *
 * 카드 자리를 따로 저장하지 않으므로 같은 컬럼 안에서의 드롭은 아무것도 바꾸지 않는다.
 * 다른 컬럼으로 옮긴 카드는 방금 수정한 카드가 되므로 목적지 맨 위에 놓는다.
 */
function buildDragMovePlan(
  currentTasks: TasksByStatus,
  result: DropResult,
): DragMovePlan | null {
  const { source, destination, draggableId } = result;
  if (!destination) return null;

  const sourceStatus = source.droppableId as TaskStatus;
  const destStatus = destination.droppableId as TaskStatus;
  if (sourceStatus === destStatus) return null;

  const movedTask = currentTasks[sourceStatus].find((task) => task.id === draggableId);
  if (!movedTask) return null;

  const updated: TasksByStatus = { ...currentTasks };
  updated[sourceStatus] = currentTasks[sourceStatus].filter((task) => task.id !== draggableId);
  updated[destStatus] = [{ ...movedTask, status: destStatus }, ...currentTasks[destStatus]];

  const doneDelta =
    (destStatus === TaskStatus.DONE ? 1 : 0) -
    (sourceStatus === TaskStatus.DONE ? 1 : 0);

  return {
    updatedTasks: updated,
    doneTotalDelta: doneDelta,
    doneOffsetDelta: doneDelta,
    persistence: { taskId: draggableId, status: destStatus },
    restore: { task: movedTask, status: sourceStatus, index: source.index },
  };
}

export default function Board({
  initialTasks,
  initialDoneTotal,
  initialDoneLimit,
  initialFocusTaskId,
  sshHosts,
  projects,
  doneAlertDismissed,
  defaultSessionType,
  vimModeEnabled = true,
}: BoardProps) {
  useAutoRefresh();
  const boardCommands = useBoardCommands();
  const t = useTranslations("board");
  const ts = useTranslations("settings");
  const tt = useTranslations("task");
  const tc = useTranslations("common");
  const locale = useLocale();
  const [tasks, setTasks] = useState<TasksByStatus>(initialTasks);
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [isProjectRegistryOpen, setIsProjectRegistryOpen] = useState(false);
  const [isBranchModalOpen, setIsBranchModalOpen] = useState(false);
  const [branchTodoDefaults, setBranchTodoDefaults] = useState<{
    baseBranch: string;
    projectId: string;
  } | null>(null);
  const [isMounted, setIsMounted] = useState(false);
  const runningAgentPanes = useRunningAgentPanes(isMounted);
  const [selectedProjectIds, setSelectedProjectIds] = useProjectFilterParams(
    projects.map((p) => p.id),
  );
  const [taskKindFilter, setTaskKindFilter] = useTaskKindFilterParams();
  const [selectedDesktops, setSelectedDesktops] = useState<string[]>([]);
  const [sortPreference, setSortPreference] = useBoardSortPreference();
  const [doneTotal, setDoneTotal] = useState(initialDoneTotal);
  const [doneOffset, setDoneOffset] = useState(initialDoneLimit);
  const [isLoadingMore, setIsLoadingMore] = useState(false);
  const [loadMoreDoneError, setLoadMoreDoneError] = useState<string | null>(null);
  const donePageGenerationRef = useRef(0);
  const loadingDonePageGenerationRef = useRef<number | null>(null);
  const [isDoneAlertDismissed, setIsDoneAlertDismissed] = useState(doneAlertDismissed);
  const [taskDeletionStateById, setTaskDeletionStateById] = useState<Record<string, TaskDeletionState>>({});
  const deletingTaskIdsRef = useRef(new Set<string>());
  const [taskMoveStateById, setTaskMoveStateById] = useState<Record<string, TaskMoveState>>({});
  const savingMoveTaskIdsRef = useRef(new Set<string>());
  /** 사용자 안내 후에 실행하기 위해 잡아 둔 Done 이동 */
  const [pendingDoneResult, setPendingDoneResult] = useState<DropResult | null>(null);
  const [currentDefaultSessionType, setCurrentDefaultSessionType] = useState<SessionType>(defaultSessionType);
  const [shouldUseMacTitlebarLayout, setShouldUseMacTitlebarLayout] = useState(false);
  const unreadNotificationCountByTask = useUnreadNotificationCountByTask();
  const notificationCenterRef = useRef<NotificationCenterButtonHandle>(null);
  const projectSelectorRef = useRef<ProjectSelectorHandle>(null);
  const hasAppliedInitialFocusRef = useRef(false);
  const pendingTaskDeleteSequenceRef = useRef<{
    taskId: string;
    timeoutId: number;
  } | null>(null);

  const projectDisplayMap = useMemo(() => buildProjectDisplayMap(projects), [projects]);

  const projectNameMap = useMemo(() => Object.fromEntries(
    [...projectDisplayMap].map(([id, display]) => [id, display.name]),
  ), [projectDisplayMap]);

  const projectGroupMap = useMemo(() => Object.fromEntries(
    [...projectDisplayMap].map(([id, display]) => [id, display.groupKey]),
  ), [projectDisplayMap]);

  const projectColorMap = useMemo(() => Object.fromEntries(
    [...projectDisplayMap].map(([id, display]) => [
      id, display.project?.color || computeProjectColor(display.name),
    ]),
  ), [projectDisplayMap]);

  const projectIconMap = useMemo(() => Object.fromEntries(
    [...projectDisplayMap].flatMap(([id, display]) => (
      display.project?.iconDataUrl ? [[id, display.project.iconDataUrl]] : []
    )),
  ), [projectDisplayMap]);

  const projectLookup = useMemo(
    () => new Map(projects.map((project) => [project.id, project])),
    [projects],
  );

  const desktopHosts = useMemo(() => Array.from(new Set([
    ...sshHosts,
    ...projects.map((project) => project.sshHost),
    ...Object.values(tasks).flat().map((task) => task.sshHost),
  ].filter((host): host is string => Boolean(host)))).sort(), [projects, sshHosts, tasks]);
  const activeDesktopFilters = useMemo(() => selectedDesktops.filter((desktop) => (
    desktop === "local" || desktopHosts.some((host) => desktop === `ssh:${host}`)
  )), [desktopHosts, selectedDesktops]);

  /** 필터 드롭다운에 표시할 메인 프로젝트 목록 (worktree 제외) */
  const filterableProjects = useMemo(
    () => projects.filter((p) => !p.isWorktree),
    [projects]
  );

  /** 선택된 프로젝트 + worktree 프로젝트 ID 집합. null이면 전체 표시 */
  const projectFilterSet = useMemo(
    () => buildProjectFilterSet(selectedProjectIds, projects),
    [selectedProjectIds, projects],
  );

  /** 선택한 보드 필터를 모두 만족하는 태스크만 표시한다 */
  const hasActiveBoardTaskFilter = projectFilterSet !== null || taskKindFilter !== "all" || activeDesktopFilters.length > 0;
  const boardTaskFilter = useCallback<BoardTaskFilter>((task) => {
    const host = task.sshHost || projectLookup.get(task.projectId ?? "")?.sshHost;
    const taskDesktop = host ? `ssh:${host}` : "local";
    return matchesProjectFilter(task, projectFilterSet)
      && matchesTaskKindFilter(task, taskKindFilter, projectLookup)
      && (activeDesktopFilters.length === 0 || activeDesktopFilters.includes(taskDesktop));
  }, [activeDesktopFilters, projectFilterSet, projectLookup, taskKindFilter]);

  const filteredTasks = useMemo(() => {
    if (!hasActiveBoardTaskFilter) return tasks;

    const filtered: TasksByStatus = {
      [TaskStatus.TODO]: [],
      [TaskStatus.PROGRESS]: [],
      [TaskStatus.PENDING]: [],
      [TaskStatus.REVIEW]: [],
      [TaskStatus.DONE]: [],
    };

    for (const status of Object.values(TaskStatus)) {
      filtered[status] = tasks[status].filter(boardTaskFilter);
    }

    return filtered;
  }, [boardTaskFilter, hasActiveBoardTaskFilter, tasks]);

  /** 프로젝트 root task의 우선순위. 같은 프로젝트의 다른 task가 이 값을 기본값으로 물려받는다 */
  const rootPriorityByProjectId = useMemo(
    () => buildProjectRootPriorityMap(
      Object.values(TaskStatus).flatMap((status) => tasks[status]),
      projectLookup,
    ),
    [projectLookup, tasks],
  );

  const boardSortContext = useMemo(
    () => ({
      rootPriorityByProjectId,
      projectNameById: new Map(Object.entries(projectNameMap)),
      /** 제목·프로젝트 정렬은 보고 있는 언어의 콜레이션을 따라야 한다. 앱은 en/ko/zh를 지원한다 */
      locale,
    }),
    [locale, projectNameMap, rootPriorityByProjectId],
  );

  /** 화면에 실제로 늘어놓을 순서. 필터를 통과한 목록에 정렬 설정을 적용한다 */
  const displayedTasks = useMemo(() => {
    if (sortPreference.keys.length === 0) return filteredTasks;

    const sorted: TasksByStatus = { ...filteredTasks };
    for (const status of Object.values(TaskStatus)) {
      sorted[status] = sortTasksForBoard(filteredTasks[status], sortPreference, boardSortContext);
    }

    return sorted;
  }, [boardSortContext, filteredTasks, sortPreference]);

  /**
   * git을 다시 돌릴 태스크. 지금 손대고 있는 칸만 새로 집계하고 나머지 칸은 마지막으로 저장된 값을 보여준다.
   * worktree와 브랜치가 있어야 baseBranch와 견줄 변경이 존재한다.
   */
  const taskIdsToRefresh = useMemo(
    () => displayedTasks[TaskStatus.PROGRESS]
      .filter((task) => task.branchName && task.worktreePath)
      .map((task) => task.id),
    [displayedTasks],
  );
  const diffStatsByTaskId = useBoardTaskDiffStats(taskIdsToRefresh, isMounted);

  const [contextMenu, setContextMenu] = useState<ContextMenuState>({
    isOpen: false,
    x: 0,
    y: 0,
    task: null,
  });
  const taskDeleteRuntimeRef = useRef({
    vimModeEnabled,
    contextMenuIsOpen: contextMenu.isOpen,
    isModalOpen,
    isProjectRegistryOpen,
    isBranchModalOpen,
    isCommandPaletteOpen: boardCommands.isCommandPaletteOpen,
    hasPendingDoneResult: Boolean(pendingDoneResult),
    displayedTasks,
    deleteConfirmMessage: tt("deleteConfirm"),
  });

  taskDeleteRuntimeRef.current = {
    vimModeEnabled,
    contextMenuIsOpen: contextMenu.isOpen,
    isModalOpen,
    isProjectRegistryOpen,
    isBranchModalOpen,
    isCommandPaletteOpen: boardCommands.isCommandPaletteOpen,
    hasPendingDoneResult: Boolean(pendingDoneResult),
    displayedTasks,
    deleteConfirmMessage: tt("deleteConfirm"),
  };

  useEffect(() => {
    setIsMounted(true);
  }, []);

  /** 서버 revalidation 후 initialTasks가 변경되면 로컬 state에 반영한다 */
  useEffect(() => {
    donePageGenerationRef.current += 1;
    loadingDonePageGenerationRef.current = null;
    setIsLoadingMore(false);
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

  useEffect(() => {
    hasAppliedInitialFocusRef.current = false;
  }, [initialFocusTaskId]);

  useEffect(() => {
    if (!isMounted || !initialFocusTaskId || hasAppliedInitialFocusRef.current) {
      return;
    }

    if (!findTaskById(displayedTasks, initialFocusTaskId)) {
      return;
    }

    const targetTaskCard = findBoardTaskCardById(initialFocusTaskId);
    if (!targetTaskCard) {
      return;
    }

    focusBoardTaskCard(targetTaskCard);
    hasAppliedInitialFocusRef.current = true;
  }, [displayedTasks, initialFocusTaskId, isMounted]);

  useEffect(() => {
    function handleWindowVimShortcut(event: KeyboardEvent) {
      if (!vimModeEnabled) return;
      if (
        contextMenu.isOpen
        || isModalOpen
        || isProjectRegistryOpen
        || isBranchModalOpen
        || pendingDoneResult
        || boardCommands.isCommandPaletteOpen
      ) return;
      if (shouldIgnoreBoardVimShortcutEvent(event)) return;

      if (isPlainVimShortcutKey(event, VIM_NEW_TASK_KEY)) {
        event.preventDefault();
        event.stopPropagation();
        setBranchTodoDefaults(null);
        setIsModalOpen(true);
        return;
      }

      if (isVimCommandShortcutKey(event)) {
        event.preventDefault();
        event.stopPropagation();
        boardCommands.openCommandPalette();
      }
    }

    window.addEventListener("keydown", handleWindowVimShortcut, true);
    return () => window.removeEventListener("keydown", handleWindowVimShortcut, true);
  }, [
    boardCommands,
    contextMenu.isOpen,
    isBranchModalOpen,
    isModalOpen,
    isProjectRegistryOpen,
    pendingDoneResult,
    vimModeEnabled,
  ]);

  useEffect(() => {
    function handleWindowTaskFocus(event: KeyboardEvent) {
      if (!isBoardTaskFocusKey(event, vimModeEnabled)) return;
      if (
        contextMenu.isOpen
        || isModalOpen
        || isProjectRegistryOpen
        || isBranchModalOpen
        || pendingDoneResult
        || boardCommands.isCommandPaletteOpen
      ) return;
      if (shouldIgnoreBoardTaskFocusEvent(event)) return;

      const firstTaskCard = getBoardTaskCards()[0];
      if (!firstTaskCard) return;

      event.preventDefault();
      focusBoardTaskCard(firstTaskCard);
    }

    window.addEventListener("keydown", handleWindowTaskFocus);
    return () => window.removeEventListener("keydown", handleWindowTaskFocus);
  }, [
    boardCommands,
    contextMenu.isOpen,
    isBranchModalOpen,
    isModalOpen,
    isProjectRegistryOpen,
    pendingDoneResult,
    vimModeEnabled,
  ]);

  const removeTaskFromBoard = useCallback((task: Pick<KanbanTask, "id" | "status">) => {
    setTasks((prev) => removeTaskFromBoardTasks(prev, task.id));
    if (task.status === TaskStatus.DONE) {
      setDoneTotal((prev) => Math.max(0, prev - 1));
      setDoneOffset((prev) => Math.max(0, prev - 1));
    }
  }, []);

  const updateTaskDeletionState = useCallback((taskId: string, deletionState: TaskDeletionState | null) => {
    setTaskDeletionStateById((prev) => setRequestStateEntry(prev, taskId, deletionState));
  }, []);

  const updateTaskMoveState = useCallback((taskId: string, moveState: TaskMoveState | null) => {
    setTaskMoveStateById((prev) => setRequestStateEntry(prev, taskId, moveState));
  }, []);

  /** 같은 task의 삭제는 한 번만 보낸다. 메뉴·키보드·재시도가 모두 이 함수를 지나므로 여기서 막는다 */
  const deleteTaskFromBoard = useCallback(async (task: Pick<KanbanTask, "id" | "status">) => {
    if (deletingTaskIdsRef.current.has(task.id)) {
      return;
    }

    deletingTaskIdsRef.current.add(task.id);
    updateTaskDeletionState(task.id, { status: "deleting" });
    try {
      const didDelete = await deleteTask(task.id);
      if (didDelete) {
        removeTaskFromBoard(task);
      }
      updateTaskDeletionState(task.id, null);
    } catch (error) {
      console.error("Failed to delete task", error);
      updateTaskDeletionState(task.id, {
        status: "failed",
        reason: error instanceof Error ? error.message : String(error),
      });
    } finally {
      deletingTaskIdsRef.current.delete(task.id);
    }
  }, [removeTaskFromBoard, updateTaskDeletionState]);

  const handleRetryTaskDelete = useCallback((task: KanbanTask) => {
    void deleteTaskFromBoard(task);
  }, [deleteTaskFromBoard]);

  useEffect(() => {
    function resetPendingTaskDeleteSequence() {
      if (pendingTaskDeleteSequenceRef.current) {
        window.clearTimeout(pendingTaskDeleteSequenceRef.current.timeoutId);
      }

      pendingTaskDeleteSequenceRef.current = null;
    }

    function armTaskDeleteSequence(taskId: string) {
      resetPendingTaskDeleteSequence();
      const timeoutId = window.setTimeout(() => {
        if (pendingTaskDeleteSequenceRef.current?.taskId === taskId) {
          pendingTaskDeleteSequenceRef.current = null;
        }
      }, TASK_DELETE_SEQUENCE_TIMEOUT_MS);

      pendingTaskDeleteSequenceRef.current = { taskId, timeoutId };
    }

    function handleWindowTaskDeleteSequence(event: KeyboardEvent) {
      const runtime = taskDeleteRuntimeRef.current;
      if (!runtime.vimModeEnabled) return;

      if (!isPlainTaskDeleteSequenceKey(event)) {
        if (!isModifierKey(event.key)) {
          resetPendingTaskDeleteSequence();
        }
        return;
      }

      if (
        event.defaultPrevented ||
        runtime.contextMenuIsOpen ||
        runtime.isModalOpen ||
        runtime.isBranchModalOpen ||
        runtime.hasPendingDoneResult ||
        runtime.isCommandPaletteOpen
      ) {
        return;
      }

      const taskCard = getTaskCardFromKeyboardEvent(event);
      const taskId = taskCard?.dataset.kanbanTaskId;
      if (!taskCard || !taskId) {
        resetPendingTaskDeleteSequence();
        return;
      }

      event.preventDefault();
      event.stopPropagation();

      if (pendingTaskDeleteSequenceRef.current?.taskId === taskId) {
        resetPendingTaskDeleteSequence();
        const task = findTaskById(runtime.displayedTasks, taskId);
        if (task && confirm(runtime.deleteConfirmMessage)) {
          void deleteTaskFromBoard(task);
        }
        return;
      }

      armTaskDeleteSequence(taskId);
    }

    window.addEventListener("keydown", handleWindowTaskDeleteSequence, true);
    return () => {
      window.removeEventListener("keydown", handleWindowTaskDeleteSequence, true);
      resetPendingTaskDeleteSequence();
    };
  }, [deleteTaskFromBoard]);

  useEffect(() => {
    function handleWindowTaskShortcut(event: KeyboardEvent) {
      const shouldOpenTaskInNewWindow = isShiftOnlyKeyboardShortcut(event, "Enter");
      const shouldOpenTaskContextMenu = isShiftOnlyKeyboardShortcut(event, "F10");
      if (!shouldOpenTaskInNewWindow && !shouldOpenTaskContextMenu) return;
      if (
        contextMenu.isOpen
        || isModalOpen
        || isProjectRegistryOpen
        || isBranchModalOpen
        || pendingDoneResult
        || boardCommands.isCommandPaletteOpen
      ) return;

      const taskCard = getTaskCardFromKeyboardEvent(event);
      const taskId = taskCard?.dataset.kanbanTaskId;
      if (!taskCard || !taskId) return;

      event.preventDefault();
      event.stopPropagation();

      if (shouldOpenTaskInNewWindow) {
        openTaskDetailInNewWindow(taskId);
        return;
      }

      const task = findTaskById(displayedTasks, taskId);
      if (!task) return;

      const rect = taskCard.getBoundingClientRect();
      setContextMenu({ isOpen: true, x: rect.left + 12, y: rect.top + 12, task });
    }

    window.addEventListener("keydown", handleWindowTaskShortcut, true);
    return () => window.removeEventListener("keydown", handleWindowTaskShortcut, true);
  }, [
    boardCommands,
    contextMenu.isOpen,
    displayedTasks,
    isBranchModalOpen,
    isModalOpen,
    isProjectRegistryOpen,
    pendingDoneResult,
  ]);

  const handleLoadMoreDone = useCallback(async () => {
    const generation = donePageGenerationRef.current;
    if (loadingDonePageGenerationRef.current === generation) return;
    loadingDonePageGenerationRef.current = generation;
    setIsLoadingMore(true);
    setLoadMoreDoneError(null);
    try {
      const result = await getMoreDoneTasks(doneOffset);
      if (generation !== donePageGenerationRef.current) return;
      setTasks((prev) => ({
        ...prev,
        [TaskStatus.DONE]: [...new Map(
          [...prev[TaskStatus.DONE], ...result.tasks].map((task) => [task.id, task]),
        ).values()],
      }));
      setDoneOffset((prev) => prev + result.tasks.length);
      setDoneTotal(result.doneTotal);
    } catch (error) {
      // 실패를 더 불러올 항목이 없는 것처럼 두지 않고 버튼 옆에 사유를 남겨 같은 페이지를 다시 요청하게 한다
      if (generation === donePageGenerationRef.current) {
        setLoadMoreDoneError(error instanceof Error ? error.message : String(error));
      }
    } finally {
      if (generation === donePageGenerationRef.current) {
        loadingDonePageGenerationRef.current = null;
        setIsLoadingMore(false);
      }
    }
  }, [doneOffset]);

  const applyDoneCountDeltas = useCallback((plan: DragMovePlan, direction: 1 | -1) => {
    if (plan.doneTotalDelta !== 0) {
      setDoneTotal((prev) => prev + plan.doneTotalDelta * direction);
    }

    if (plan.doneOffsetDelta !== 0) {
      setDoneOffset((prev) => prev + plan.doneOffsetDelta * direction);
    }
  }, []);

  /**
   * 카드를 먼저 옮기고 저장한다. 저장하는 동안 카드에 저장 중을 보이고 같은 카드의 이동을 막으며,
   * 실패하면 카드를 원래 자리로 돌리고 사유와 다시 옮기기를 남긴다.
   */
  const executeDragMove = useCallback(
    (result: DropResult) => {
      const plan = buildDragMovePlan(tasks, result);
      if (!plan) return;

      const { taskId, status: targetStatus } = plan.persistence;
      if (savingMoveTaskIdsRef.current.has(taskId)) return;

      savingMoveTaskIdsRef.current.add(taskId);
      setTasks(plan.updatedTasks);
      applyDoneCountDeltas(plan, 1);
      updateTaskMoveState(taskId, { status: "saving" });

      void (async () => {
        try {
          await moveTaskToColumn(taskId, targetStatus);
          updateTaskMoveState(taskId, null);
        } catch (error) {
          console.error("Failed to move task", error);
          setTasks((prev) => restoreMovedTask(prev, plan));
          applyDoneCountDeltas(plan, -1);
          updateTaskMoveState(taskId, {
            status: "failed",
            targetStatus,
            reason: error instanceof Error ? error.message : String(error),
          });
        } finally {
          savingMoveTaskIdsRef.current.delete(taskId);
        }
      })();
    },
    [applyDoneCountDeltas, tasks, updateTaskMoveState]
  );

  const moveTaskToStatus = useCallback(
    (task: KanbanTask, newStatus: TaskStatus) => {
      const result = buildStatusMoveResult(task, newStatus, tasks);
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
    [executeDragMove, displayedTasks, isDoneAlertDismissed, tasks],
  );

  const handleRetryTaskMove = useCallback((task: KanbanTask) => {
    const moveState = taskMoveStateById[task.id];
    if (moveState?.status === "failed") {
      moveTaskToStatus(task, moveState.targetStatus);
    }
  }, [moveTaskToStatus, taskMoveStateById]);

  const moveFocusedTaskToStatus = useCallback(
    (newStatus: TaskStatus, taskIdOverride?: string | null): "moved" | "missing-focus" | "missing-task" => {
      const taskCard = getFocusedBoardTaskCard();
      const taskId = taskIdOverride ?? taskCard?.dataset.kanbanTaskId;
      if (!taskId) return "missing-focus";

      const task = findTaskById(displayedTasks, taskId);
      if (!task) return "missing-task";

      moveTaskToStatus(task, newStatus);
      return "moved";
    },
    [displayedTasks, moveTaskToStatus],
  );

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
    moveFocusedTaskToStatus,
  }), [boardCommands, moveFocusedTaskToStatus]);

  const handleDragEnd = useCallback(
    (result: DropResult) => {
      const { source, destination, draggableId } = result;
      if (!destination) return;

      const sourceStatus = source.droppableId as TaskStatus;
      const destStatus = destination.droppableId as TaskStatus;

      /** Done 이동 시 리소스 보존 안내 (dismissed 아닌 경우만) */
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
    const task = contextMenu.task;
    if (task && confirm(tt("deleteConfirm"))) {
      void deleteTaskFromBoard(task);
    }
    handleCloseContextMenu();
  }, [contextMenu.task, deleteTaskFromBoard, handleCloseContextMenu, tt]);

  const handleStatusChangeFromCard = useCallback(
    (newStatus: TaskStatus) => {
      const task = contextMenu.task;
      handleCloseContextMenu();
      if (!task) return;

      moveTaskToStatus(task, newStatus);
    },
    [contextMenu.task, handleCloseContextMenu, moveTaskToStatus],
  );

  const headerClassName = shouldUseMacTitlebarLayout
    ? "flex items-center justify-end bg-bg-page px-6 pb-3 pl-20 pr-6 pt-10 [-webkit-app-region:drag]"
    : "flex items-center justify-end border-b border-border-default bg-bg-surface px-6 py-3";

  const mainClassName = shouldUseMacTitlebarLayout ? "px-6 pb-6" : "p-6";

  return (
    <div className="min-h-screen bg-bg-page">
      <BoardPageFindBar vimModeEnabled={vimModeEnabled} />
      <header className={headerClassName}>
        <img
          src="./branding/kanvibe-logo.png"
          alt="KanVibe"
          width={118}
          height={32}
          className="kanvibe-brand-logo mr-auto hidden h-8 w-auto shrink-0 pr-6 xl:block"
        />
        <div className="flex flex-wrap items-center justify-end gap-3 [-webkit-app-region:no-drag]">
          <div
            role="group"
            aria-label={t("taskKindFilter.label")}
            className="flex h-[34px] w-[180px] shrink-0 items-stretch rounded-md border border-border-default bg-bg-page p-0.5"
            data-testid="task-kind-filter"
          >
            {TASK_KIND_FILTER_VALUES.map((filter) => {
              const isActive = taskKindFilter === filter;
              return (
                <button
                  key={filter}
                  type="button"
                  aria-pressed={isActive}
                  title={t(`taskKindFilter.descriptions.${filter}`)}
                  onClick={() => setTaskKindFilter(filter)}
                  className={`flex flex-1 items-center justify-center rounded text-sm font-medium transition-colors ${
                    isActive
                      ? "bg-brand-primary text-text-inverse shadow-sm"
                      : "text-text-secondary hover:bg-bg-surface hover:text-text-primary"
                  }`}
                >
                  {t(`taskKindFilter.options.${filter}`)}
                </button>
              );
            })}
          </div>
          <div className="w-64" data-testid="project-filter-control">
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
          <DesktopFilter hosts={desktopHosts} selectedDesktops={activeDesktopFilters} onChange={setSelectedDesktops} />
          <BoardSortPicker preference={sortPreference} onChange={setSortPreference} />
          <button
            type="button"
            onClick={() => setIsProjectRegistryOpen(true)}
            title={ts("scanTitle")}
            aria-label={ts("scanTitle")}
            className="flex shrink-0 items-center justify-center rounded-md border border-border-default bg-bg-surface p-1.5 text-text-secondary transition-colors hover:border-brand-primary hover:text-text-primary"
          >
            <svg
              aria-hidden="true"
              width="20"
              height="20"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.93a2 2 0 0 1-1.66-.9l-.82-1.2A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />
              <path d="M12 10v6" />
              <path d="M9 13h6" />
            </svg>
          </button>
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
                  tasks={displayedTasks[col.status]}
                  label={t(`columns.${col.labelKey}`)}
                  colorClass={col.colorClass}
                  onContextMenu={handleContextMenu}
                  projectNameMap={projectNameMap}
                  projectGroupMap={projectGroupMap}
                  projectColorMap={projectColorMap}
                  projectIconMap={projectIconMap}
                  unreadNotificationCountByTask={unreadNotificationCountByTask}
                  rootPriorityByProjectId={rootPriorityByProjectId}
                  vimModeEnabled={vimModeEnabled}
                  runningAgentPanes={runningAgentPanes}
                  diffStatsByTaskId={diffStatsByTaskId}
                  taskDeletionStateById={taskDeletionStateById}
                  onRetryTaskDelete={handleRetryTaskDelete}
                  taskMoveStateById={taskMoveStateById}
                  onRetryTaskMove={handleRetryTaskMove}
                  {...(col.status === TaskStatus.DONE && {
                    totalCount: doneTotal,
                    hasMore: doneOffset < doneTotal,
                    onLoadMore: handleLoadMoreDone,
                    isLoadingMore,
                    loadMoreError: loadMoreDoneError,
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

      {isProjectRegistryOpen ? (
        <ProjectRegistryDialog
          isOpen
          onClose={() => setIsProjectRegistryOpen(false)}
          projects={projects}
          sshHosts={sshHosts}
        />
      ) : null}

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
