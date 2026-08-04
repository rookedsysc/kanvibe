"use client";

import { useState, useCallback, useEffect, useMemo, useRef, useTransition } from "react";
import { useTranslations } from "next-intl";
import { DragDropContext, type DropResult } from "@hello-pangea/dnd";
import BoardPageFindBar from "./BoardPageFindBar";
import Column from "./Column";
import CreateTaskModal from "./CreateTaskModal";
import ProjectRegistryDialog from "./ProjectRegistryDialog";
import NotificationCenterButton from "./NotificationCenterButton";
import ProjectSelector from "./ProjectSelector";
import TaskContextMenu from "./TaskContextMenu";
import BranchTaskModal from "./BranchTaskModal";
import DoneConfirmDialog from "./DoneConfirmDialog";
import { reorderTasks, deleteTask, getMoreDoneTasks, moveTaskToColumn } from "@/desktop/renderer/actions/kanban";
import { runBackgroundTaskSyncNow } from "@/desktop/renderer/actions/backgroundTaskSync";
import type { TasksByStatus } from "@/desktop/renderer/actions/kanban";
import { useBoardCommands } from "@/desktop/renderer/components/BoardCommandProvider";
import { navigateToTaskDetail } from "@/desktop/renderer/utils/taskNavigation";
import { SessionType, TaskStatus, type KanbanTask } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";
import { useAutoRefresh } from "@/desktop/renderer/hooks/useAutoRefresh";
import { useProjectFilterParams } from "@/desktop/renderer/hooks/useProjectFilterParams";
import {
  TASK_KIND_FILTER_VALUES,
  useTaskKindFilterParams,
  type TaskKindFilter,
} from "@/desktop/renderer/hooks/useTaskKindFilterParams";
import { DEFAULT_LOCALE, SUPPORTED_LOCALES } from "@/desktop/renderer/utils/locales";
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

const VIM_MOVE_COMMAND_ALIASES = new Set(["move", "m"]);
const VIM_SYNC_COMMAND_ALIASES = new Set(["sync", "s"]);
const VIM_COMMAND_NAMES = ["move", "sync"] as const;
const VIM_MOVE_STATUSES = [
  TaskStatus.TODO,
  TaskStatus.PROGRESS,
  TaskStatus.PENDING,
  TaskStatus.REVIEW,
  TaskStatus.DONE,
] as const;

const VIM_STATUS_ALIASES: Record<string, TaskStatus> = {
  t: TaskStatus.TODO,
  todo: TaskStatus.TODO,
  "to-do": TaskStatus.TODO,
  progress: TaskStatus.PROGRESS,
  prog: TaskStatus.PROGRESS,
  doing: TaskStatus.PROGRESS,
  pending: TaskStatus.PENDING,
  pend: TaskStatus.PENDING,
  review: TaskStatus.REVIEW,
  rev: TaskStatus.REVIEW,
  done: TaskStatus.DONE,
  d: TaskStatus.DONE,
};

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

function getFocusedBoardTaskCard() {
  const activeElement = document.activeElement;
  return activeElement instanceof Element
    ? activeElement.closest<HTMLAnchorElement>(TASK_CARD_SELECTOR)
    : null;
}

type ParsedVimCommand =
  | { type: "move"; destinationStatus: TaskStatus }
  | { type: "sync" };

function parseVimCommandTokens(commandValue: string) {
  return commandValue
    .trim()
    .replace(/^:/, "")
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);
}

function parseVimCommand(commandValue: string): ParsedVimCommand | null {
  const tokens = parseVimCommandTokens(commandValue);

  if (tokens.length === 1 && VIM_SYNC_COMMAND_ALIASES.has(tokens[0])) {
    return { type: "sync" };
  }

  if (tokens.length === 2 && VIM_MOVE_COMMAND_ALIASES.has(tokens[0])) {
    const destinationStatus = VIM_STATUS_ALIASES[tokens[1]];
    return destinationStatus ? { type: "move", destinationStatus } : null;
  }

  return null;
}

function getUniqueVimMoveStatusCompletion(statusPrefix: string): TaskStatus | null {
  const normalizedPrefix = statusPrefix.toLowerCase();
  if (!normalizedPrefix) return null;

  const canonicalMatches = VIM_MOVE_STATUSES.filter((status) => status.startsWith(normalizedPrefix));
  if (canonicalMatches.length === 1) return canonicalMatches[0];
  if (canonicalMatches.length > 1) return null;

  const aliasMatches = new Set<TaskStatus>();
  for (const [alias, status] of Object.entries(VIM_STATUS_ALIASES)) {
    if (alias.startsWith(normalizedPrefix)) {
      aliasMatches.add(status);
    }
  }

  return aliasMatches.size === 1 ? Array.from(aliasMatches)[0] : null;
}

function getUniqueVimCommandCompletion(commandPrefix: string) {
  const normalizedPrefix = commandPrefix.toLowerCase();
  if (!normalizedPrefix) return "move";

  const matches = VIM_COMMAND_NAMES.filter((command) => command.startsWith(normalizedPrefix));
  return matches.length === 1 ? matches[0] : null;
}

function getVimCommandAutocomplete(commandValue: string): string | null {
  const leadingWhitespace = commandValue.match(/^\s*/)?.[0] ?? "";
  const trimmedStartValue = commandValue.trimStart();
  const hasColonPrefix = trimmedStartValue.startsWith(":");
  const valueWithoutColon = hasColonPrefix ? trimmedStartValue.slice(1) : trimmedStartValue;
  const hasTrailingWhitespace = /\s$/.test(valueWithoutColon);
  const tokens = valueWithoutColon.toLowerCase().split(/\s+/).filter(Boolean);
  const commandToken = tokens[0] ?? "";

  const commandPrefix = `${leadingWhitespace}${hasColonPrefix ? ":" : ""}`;
  const formatMoveCompletion = (status?: TaskStatus) => {
    const prefix = `${commandPrefix}move`;
    return status ? `${prefix} ${status}` : `${prefix} `;
  };
  const formatCommandCompletion = (command: typeof VIM_COMMAND_NAMES[number]) => {
    return command === "move" ? formatMoveCompletion() : `${commandPrefix}${command}`;
  };

  if (tokens.length === 0) {
    return formatMoveCompletion();
  }

  if (tokens.length === 1 && !hasTrailingWhitespace) {
    const completedCommand = getUniqueVimCommandCompletion(commandToken);
    if (!completedCommand) return null;

    const completion = formatCommandCompletion(completedCommand);
    return completion === commandValue ? null : completion;
  }

  if (!VIM_MOVE_COMMAND_ALIASES.has(commandToken) || tokens.length !== 2) {
    return null;
  }

  const completedStatus = getUniqueVimMoveStatusCompletion(tokens[1]);
  if (!completedStatus) return null;

  const completion = formatMoveCompletion(completedStatus);
  return completion === commandValue ? null : completion;
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

function isProjectRootTask(task: KanbanTask, projectLookup: Map<string, Project>): boolean {
  if (!task.projectId || !task.branchName) return false;

  const project = projectLookup.get(task.projectId);
  return Boolean(project && !project.isWorktree && task.branchName === project.defaultBranch);
}

function matchesTaskKindFilter(
  task: KanbanTask,
  taskKindFilter: TaskKindFilter,
  projectLookup: Map<string, Project>,
): boolean {
  if (taskKindFilter === "all") return true;

  const isRootTask = isProjectRootTask(task, projectLookup);
  return taskKindFilter === "project" ? isRootTask : !isRootTask;
}

function matchesProjectFilter(task: KanbanTask, projectFilterSet: Set<string> | null): boolean {
  return !projectFilterSet || Boolean(task.projectId && projectFilterSet.has(task.projectId));
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
  taskFilter: BoardTaskFilter | null
): KanbanTask[] {
  const arr = [...fullArray];

  if (!taskFilter) {
    arr.splice(filteredIndex, 0, task);
    return arr;
  }

  const filtered = arr.filter(taskFilter);

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
  taskFilter: BoardTaskFilter | null,
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
      taskFilter,
    );

    const orderedIds = (
      taskFilter
        ? updated[sourceStatus].filter(taskFilter)
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
    taskFilter,
  );

  const orderedIds = (
    taskFilter
      ? updated[destStatus].filter(taskFilter)
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
  const [tasks, setTasks] = useState<TasksByStatus>(initialTasks);
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [isProjectRegistryOpen, setIsProjectRegistryOpen] = useState(false);
  const [isBranchModalOpen, setIsBranchModalOpen] = useState(false);
  const [isVimCommandOpen, setIsVimCommandOpen] = useState(false);
  const [vimCommandValue, setVimCommandValue] = useState("");
  const [vimCommandError, setVimCommandError] = useState<string | null>(null);
  const [vimCommandTaskId, setVimCommandTaskId] = useState<string | null>(null);
  const [branchTodoDefaults, setBranchTodoDefaults] = useState<{
    baseBranch: string;
    projectId: string;
  } | null>(null);
  const [isMounted, setIsMounted] = useState(false);
  const [selectedProjectIds, setSelectedProjectIds] = useProjectFilterParams(
    projects.map((p) => p.id),
  );
  const [taskKindFilter, setTaskKindFilter] = useTaskKindFilterParams();
  const [doneTotal, setDoneTotal] = useState(initialDoneTotal);
  const [doneOffset, setDoneOffset] = useState(initialDoneLimit);
  const [isLoadingMore, setIsLoadingMore] = useState(false);
  const [isDoneAlertDismissed, setIsDoneAlertDismissed] = useState(doneAlertDismissed);
  const [pendingDoneResult, setPendingDoneResult] = useState<DropResult | null>(null);
  const [currentDefaultSessionType, setCurrentDefaultSessionType] = useState<SessionType>(defaultSessionType);
  const [shouldUseMacTitlebarLayout, setShouldUseMacTitlebarLayout] = useState(false);
  const vimCommandCompletion = useMemo(
    () => getVimCommandAutocomplete(vimCommandValue),
    [vimCommandValue],
  );
  const [, startDragPersistenceTransition] = useTransition();
  const notificationCenterRef = useRef<NotificationCenterButtonHandle>(null);
  const projectSelectorRef = useRef<ProjectSelectorHandle>(null);
  const vimCommandInputRef = useRef<HTMLInputElement>(null);
  const hasAppliedInitialFocusRef = useRef(false);
  const pendingTaskDeleteSequenceRef = useRef<{
    taskId: string;
    timeoutId: number;
  } | null>(null);

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

  /** 프로젝트명 → 해당 이름의 메인 프로젝트(worktree 제외) 매핑 */
  const mainProjectByName = useMemo(() => {
    const uniqueNames = new Set(Object.values(projectNameMap));
    const mainProjects = new Map<string, Project>();

    for (const name of uniqueNames) {
      const mainProject = projects.find(
        (p) => p.name === name && !extractMainRepoPath(p.repoPath)
      );
      if (mainProject) {
        mainProjects.set(name, mainProject);
      }
    }

    return mainProjects;
  }, [projectNameMap, projects]);

  /** 프로젝트명 → hex 색상 매핑. DB color 우선, 없으면 해시 기반 프리셋 할당 */
  const projectColorMap = useMemo(() => {
    const colorMap: Record<string, string> = {};

    for (const name of new Set(Object.values(projectNameMap))) {
      colorMap[name] = mainProjectByName.get(name)?.color || computeProjectColor(name);
    }
    return colorMap;
  }, [projectNameMap, mainProjectByName]);

  /** 프로젝트명 → GitHub 아이콘 data URL 매핑. 아이콘이 없는 프로젝트는 항목을 만들지 않는다 */
  const projectIconMap = useMemo(() => {
    const iconMap: Record<string, string> = {};

    for (const name of new Set(Object.values(projectNameMap))) {
      const iconDataUrl = mainProjectByName.get(name)?.iconDataUrl;
      if (iconDataUrl) {
        iconMap[name] = iconDataUrl;
      }
    }
    return iconMap;
  }, [projectNameMap, mainProjectByName]);

  const projectLookup = useMemo(
    () => new Map(projects.map((project) => [project.id, project])),
    [projects],
  );

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

  /** 프로젝트 + task kind 필터가 적용된 태스크 목록 */
  const hasActiveBoardTaskFilter = projectFilterSet !== null || taskKindFilter !== "all";
  const boardTaskFilter = useCallback<BoardTaskFilter>((task) => (
    matchesProjectFilter(task, projectFilterSet)
    && matchesTaskKindFilter(task, taskKindFilter, projectLookup)
  ), [projectFilterSet, projectLookup, taskKindFilter]);

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
    isVimCommandOpen,
    hasPendingDoneResult: Boolean(pendingDoneResult),
    filteredTasks,
    deleteConfirmMessage: tt("deleteConfirm"),
  });

  taskDeleteRuntimeRef.current = {
    vimModeEnabled,
    contextMenuIsOpen: contextMenu.isOpen,
    isModalOpen,
    isProjectRegistryOpen,
    isBranchModalOpen,
    isVimCommandOpen,
    hasPendingDoneResult: Boolean(pendingDoneResult),
    filteredTasks,
    deleteConfirmMessage: tt("deleteConfirm"),
  };

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

  useEffect(() => {
    hasAppliedInitialFocusRef.current = false;
  }, [initialFocusTaskId]);

  useEffect(() => {
    if (!isMounted || !initialFocusTaskId || hasAppliedInitialFocusRef.current) {
      return;
    }

    if (!findTaskById(filteredTasks, initialFocusTaskId)) {
      return;
    }

    const targetTaskCard = findBoardTaskCardById(initialFocusTaskId);
    if (!targetTaskCard) {
      return;
    }

    focusBoardTaskCard(targetTaskCard);
    hasAppliedInitialFocusRef.current = true;
  }, [filteredTasks, initialFocusTaskId, isMounted]);

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
    boardCommands.setTaskQuickSearchOpen(isVimCommandOpen);
    return () => boardCommands.setTaskQuickSearchOpen(false);
  }, [boardCommands, isVimCommandOpen]);

  useEffect(() => {
    if (isVimCommandOpen) {
      vimCommandInputRef.current?.focus();
    }
  }, [isVimCommandOpen]);

  useEffect(() => {
    function handleWindowVimShortcut(event: KeyboardEvent) {
      if (!vimModeEnabled) return;
      if (contextMenu.isOpen || isModalOpen || isProjectRegistryOpen || isBranchModalOpen || pendingDoneResult || isVimCommandOpen) return;
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
        setVimCommandValue("");
        setVimCommandError(null);
        setVimCommandTaskId(getFocusedBoardTaskCard()?.dataset.kanbanTaskId ?? null);
        setIsVimCommandOpen(true);
      }
    }

    window.addEventListener("keydown", handleWindowVimShortcut, true);
    return () => window.removeEventListener("keydown", handleWindowVimShortcut, true);
  }, [contextMenu.isOpen, isBranchModalOpen, isModalOpen, isProjectRegistryOpen, isVimCommandOpen, pendingDoneResult, vimModeEnabled]);

  useEffect(() => {
    function handleWindowTaskFocus(event: KeyboardEvent) {
      if (!isBoardTaskFocusKey(event, vimModeEnabled)) return;
      if (contextMenu.isOpen || isModalOpen || isProjectRegistryOpen || isBranchModalOpen || pendingDoneResult || isVimCommandOpen) return;
      if (shouldIgnoreBoardTaskFocusEvent(event)) return;

      const firstTaskCard = getBoardTaskCards()[0];
      if (!firstTaskCard) return;

      event.preventDefault();
      focusBoardTaskCard(firstTaskCard);
    }

    window.addEventListener("keydown", handleWindowTaskFocus);
    return () => window.removeEventListener("keydown", handleWindowTaskFocus);
  }, [contextMenu.isOpen, isBranchModalOpen, isModalOpen, isProjectRegistryOpen, isVimCommandOpen, pendingDoneResult, vimModeEnabled]);

  const removeTaskFromBoard = useCallback((task: Pick<KanbanTask, "id" | "status">) => {
    setTasks((prev) => removeTaskFromBoardTasks(prev, task.id));
    if (task.status === TaskStatus.DONE) {
      setDoneTotal((prev) => Math.max(0, prev - 1));
      setDoneOffset((prev) => Math.max(0, prev - 1));
    }
  }, []);

  const deleteTaskFromBoard = useCallback(async (task: Pick<KanbanTask, "id" | "status">) => {
    try {
      const didDelete = await deleteTask(task.id);
      if (didDelete) {
        removeTaskFromBoard(task);
      }
    } catch (error) {
      console.error("Failed to delete task", error);
    }
  }, [removeTaskFromBoard]);

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
        runtime.isVimCommandOpen
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
        const task = findTaskById(runtime.filteredTasks, taskId);
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
      if (contextMenu.isOpen || isModalOpen || isProjectRegistryOpen || isBranchModalOpen || pendingDoneResult || isVimCommandOpen) return;

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
  }, [contextMenu.isOpen, filteredTasks, isBranchModalOpen, isModalOpen, isProjectRegistryOpen, isVimCommandOpen, pendingDoneResult]);

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
      const plan = buildDragMovePlan(
        tasks,
        result,
        hasActiveBoardTaskFilter ? boardTaskFilter : null,
      );
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
    [boardTaskFilter, hasActiveBoardTaskFilter, startDragPersistenceTransition, tasks]
  );

  const moveTaskToStatus = useCallback(
    (task: KanbanTask, newStatus: TaskStatus) => {
      const result = buildStatusMoveResult(task, newStatus, tasks, filteredTasks);
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
    [executeDragMove, filteredTasks, isDoneAlertDismissed, tasks],
  );

  const moveFocusedTaskToStatus = useCallback(
    (newStatus: TaskStatus, taskIdOverride?: string | null): "moved" | "missing-focus" | "missing-task" => {
      const taskCard = getFocusedBoardTaskCard();
      const taskId = taskIdOverride ?? taskCard?.dataset.kanbanTaskId;
      if (!taskId) return "missing-focus";

      const task = findTaskById(filteredTasks, taskId);
      if (!task) return "missing-task";

      moveTaskToStatus(task, newStatus);
      return "moved";
    },
    [filteredTasks, moveTaskToStatus],
  );

  const closeVimCommand = useCallback(() => {
    setIsVimCommandOpen(false);
    setVimCommandValue("");
    setVimCommandError(null);
    setVimCommandTaskId(null);
  }, []);

  const submitVimCommand = useCallback(() => {
    const parsedCommand = parseVimCommand(vimCommandValue);
    if (!parsedCommand) {
      setVimCommandError(t("vimCommand.errors.unknownCommand"));
      return;
    }

    if (parsedCommand.type === "sync") {
      closeVimCommand();
      void runBackgroundTaskSyncNow().catch((error) => {
        console.error("Failed to run background task sync", error);
      });
      return;
    }

    const moveResult = moveFocusedTaskToStatus(parsedCommand.destinationStatus, vimCommandTaskId);
    if (moveResult === "missing-focus") {
      setVimCommandError(t("vimCommand.errors.noFocusedTask"));
      return;
    }
    if (moveResult === "missing-task") {
      setVimCommandError(t("vimCommand.errors.taskNotFound"));
      return;
    }

    closeVimCommand();
  }, [closeVimCommand, moveFocusedTaskToStatus, t, vimCommandTaskId, vimCommandValue]);

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
        <div className="flex items-center gap-3 [-webkit-app-region:no-drag]">
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
                  tasks={filteredTasks[col.status]}
                  label={t(`columns.${col.labelKey}`)}
                  colorClass={col.colorClass}
                  onContextMenu={handleContextMenu}
                  projectNameMap={projectNameMap}
                  projectColorMap={projectColorMap}
                  projectIconMap={projectIconMap}
                  vimModeEnabled={vimModeEnabled}
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

      {isVimCommandOpen && (
        <div className="fixed bottom-4 left-1/2 z-[350] w-[min(520px,calc(100vw-32px))] -translate-x-1/2 rounded-lg border border-border-default bg-bg-surface p-3 shadow-lg">
          <label htmlFor="vim-command-input" className="sr-only">
            {t("vimCommand.label")}
          </label>
          <div className="flex items-center gap-2 rounded-md border border-border-default bg-bg-page px-3 py-2 text-sm text-text-primary focus-within:border-brand-primary">
            <span className="font-mono text-text-muted" aria-hidden="true">:</span>
            <input
              id="vim-command-input"
              ref={vimCommandInputRef}
              value={vimCommandValue}
              onChange={(event) => {
                setVimCommandValue(event.target.value);
                setVimCommandError(null);
              }}
              onKeyDown={(event) => {
                if (event.key === "Tab" && !event.shiftKey && !event.altKey && !event.ctrlKey && !event.metaKey) {
                  const completion = getVimCommandAutocomplete(event.currentTarget.value);
                  if (completion) {
                    event.preventDefault();
                    setVimCommandValue(completion);
                    setVimCommandError(null);
                  }
                  return;
                }
                if (event.key === "Enter") {
                  event.preventDefault();
                  submitVimCommand();
                  return;
                }
                if (event.key === "Escape") {
                  event.preventDefault();
                  closeVimCommand();
                }
              }}
              aria-label={t("vimCommand.label")}
              data-shortcut-capture="true"
              className="min-w-0 flex-1 bg-transparent font-mono outline-none placeholder:text-text-muted"
              placeholder={t("vimCommand.placeholder")}
            />
          </div>
          <p className={`mt-2 text-xs ${vimCommandError ? "text-status-error" : "text-text-muted"}`}>
            {vimCommandError
              ?? (vimCommandCompletion
                ? t("vimCommand.completionHint", { command: `:${vimCommandCompletion.trim().replace(/^:/, "")}` })
                : t("vimCommand.hint"))}
          </p>
        </div>
      )}

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
