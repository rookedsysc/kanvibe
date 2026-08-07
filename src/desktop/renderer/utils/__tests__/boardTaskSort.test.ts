import { describe, expect, it } from "vitest";
import {
  buildProjectRootPriorityMap,
  isInheritedPriority,
  resolveEffectivePriority,
  sortTasksForBoard,
  type BoardSortContext,
} from "@/desktop/renderer/utils/boardTaskSort";
import { TaskStatus, type KanbanTask } from "@/entities/KanbanTask";
import { TaskPriority } from "@/entities/TaskPriority";
import type { Project } from "@/entities/Project";
import type { BoardSortPreference } from "@/desktop/shared/boardSort";

function buildTask(overrides: Partial<KanbanTask> & { id: string }): KanbanTask {
  return {
    title: overrides.id,
    description: null,
    status: TaskStatus.TODO,
    branchName: null,
    worktreePath: null,
    sessionType: null,
    sessionName: null,
    sshHost: null,
    agentType: null,
    project: null,
    projectId: null,
    baseBranch: null,
    prUrl: null,
    priority: null,
    displayRank: "8",
    isManuallyOrdered: false,
    createdAt: new Date("2026-01-01T00:00:00Z"),
    updatedAt: new Date("2026-01-01T00:00:00Z"),
    ...overrides,
  } as KanbanTask;
}

function buildProject(overrides: Partial<Project> & { id: string }): Project {
  return {
    name: overrides.id,
    repoPath: `/repo/${overrides.id}`,
    defaultBranch: "main",
    sshHost: null,
    isWorktree: false,
    color: null,
    iconDataUrl: null,
    createdAt: new Date("2026-01-01T00:00:00Z"),
    ...overrides,
  } as Project;
}

const EMPTY_CONTEXT: BoardSortContext = {
  rootPriorityByProjectId: new Map(),
  projectNameById: new Map(),
};

function preference(overrides: Partial<BoardSortPreference>): BoardSortPreference {
  return { keys: [], mode: "sort-first", ...overrides };
}

describe("resolveEffectivePriority", () => {
  it("task에 우선순위가 없으면 프로젝트 root task의 우선순위를 물려받는다", () => {
    // Given
    const task = buildTask({ id: "child", projectId: "project-1" });
    const rootPriorityByProjectId = new Map([["project-1", TaskPriority.HIGH]]);

    // When / Then
    expect(resolveEffectivePriority(task, rootPriorityByProjectId)).toBe(TaskPriority.HIGH);
    expect(isInheritedPriority(task, rootPriorityByProjectId)).toBe(true);
  });

  it("task가 자기 우선순위를 가지면 물려받은 값을 덮어쓴다", () => {
    // Given
    const task = buildTask({ id: "child", projectId: "project-1", priority: TaskPriority.LOW });
    const rootPriorityByProjectId = new Map([["project-1", TaskPriority.HIGH]]);

    // When / Then
    expect(resolveEffectivePriority(task, rootPriorityByProjectId)).toBe(TaskPriority.LOW);
    expect(isInheritedPriority(task, rootPriorityByProjectId)).toBe(false);
  });

  it("물려받을 root task가 없거나 프로젝트가 없으면 우선순위가 없다", () => {
    // Given
    const orphanTask = buildTask({ id: "orphan" });
    const otherProjectTask = buildTask({ id: "other", projectId: "project-2" });
    const rootPriorityByProjectId = new Map([["project-1", TaskPriority.HIGH]]);

    // When / Then
    expect(resolveEffectivePriority(orphanTask, rootPriorityByProjectId)).toBeNull();
    expect(resolveEffectivePriority(otherProjectTask, rootPriorityByProjectId)).toBeNull();
  });
});

describe("buildProjectRootPriorityMap", () => {
  it("기본 브랜치를 든 task만 root로 보고 그 우선순위를 모은다", () => {
    // Given
    const projectLookup = new Map([["project-1", buildProject({ id: "project-1" })]]);
    const tasks = [
      buildTask({ id: "root", projectId: "project-1", branchName: "main", priority: TaskPriority.MEDIUM }),
      buildTask({ id: "child", projectId: "project-1", branchName: "feat/a", priority: TaskPriority.HIGH }),
    ];

    // When
    const rootPriorityByProjectId = buildProjectRootPriorityMap(tasks, projectLookup);

    // Then
    expect(rootPriorityByProjectId.get("project-1")).toBe(TaskPriority.MEDIUM);
  });

  it("worktree 프로젝트는 root 취급하지 않는다", () => {
    // Given
    const projectLookup = new Map([
      ["worktree-1", buildProject({ id: "worktree-1", isWorktree: true })],
    ]);
    const tasks = [
      buildTask({ id: "root", projectId: "worktree-1", branchName: "main", priority: TaskPriority.HIGH }),
    ];

    // When / Then
    expect(buildProjectRootPriorityMap(tasks, projectLookup).size).toBe(0);
  });
});

describe("sortTasksForBoard", () => {
  const highTask = buildTask({ id: "high", title: "c", priority: TaskPriority.HIGH, displayRank: "6" });
  const mediumTask = buildTask({ id: "medium", title: "a", priority: TaskPriority.MEDIUM, displayRank: "4" });
  const noPriorityTask = buildTask({ id: "none", title: "b", displayRank: "2" });

  it("기준이 하나도 없으면 들어온 순서를 그대로 둔다", () => {
    // Given
    const tasks = [highTask, mediumTask, noPriorityTask];

    // When
    const sorted = sortTasksForBoard(tasks, preference({}), EMPTY_CONTEXT);

    // Then
    expect(sorted).toBe(tasks);
  });

  it("앞선 기준이 같을 때만 다음 기준이 개입한다", () => {
    // Given
    const firstTask = buildTask({ id: "first", title: "b", priority: TaskPriority.HIGH });
    const secondTask = buildTask({ id: "second", title: "a", priority: TaskPriority.HIGH });
    const lastTask = buildTask({ id: "last", title: "a", priority: TaskPriority.LOW });

    // When
    const sorted = sortTasksForBoard(
      [firstTask, secondTask, lastTask],
      preference({
        keys: [
          { field: "priority", direction: "asc" },
          { field: "title", direction: "asc" },
        ],
      }),
      EMPTY_CONTEXT,
    );

    // Then
    expect(sorted.map((task) => task.id)).toEqual(["second", "first", "last"]);
  });

  it("방향을 뒤집어도 값이 없는 항목은 계속 뒤에 남는다", () => {
    // Given
    const tasks = [noPriorityTask, mediumTask, highTask];

    // When
    const ascending = sortTasksForBoard(
      tasks,
      preference({ keys: [{ field: "priority", direction: "asc" }] }),
      EMPTY_CONTEXT,
    );
    const descending = sortTasksForBoard(
      tasks,
      preference({ keys: [{ field: "priority", direction: "desc" }] }),
      EMPTY_CONTEXT,
    );

    // Then
    expect(ascending.map((task) => task.id)).toEqual(["high", "medium", "none"]);
    expect(descending.map((task) => task.id)).toEqual(["medium", "high", "none"]);
  });

  it("물려받은 우선순위도 정렬에 그대로 쓰인다", () => {
    // Given
    const inheritingTask = buildTask({ id: "inheriting", projectId: "project-1" });
    const context: BoardSortContext = {
      rootPriorityByProjectId: new Map([["project-1", TaskPriority.HIGH]]),
      projectNameById: new Map(),
    };

    // When
    const sorted = sortTasksForBoard(
      [mediumTask, inheritingTask],
      preference({ keys: [{ field: "priority", direction: "asc" }] }),
      context,
    );

    // Then
    expect(sorted.map((task) => task.id)).toEqual(["inheriting", "medium"]);
  });

  describe("수동 배치 순서와 겹치는 방식", () => {
    const pinnedLast = buildTask({ id: "pinned-last", priority: TaskPriority.LOW, displayRank: "6", isManuallyOrdered: true });
    const pinnedFirst = buildTask({ id: "pinned-first", priority: TaskPriority.LOW, displayRank: "2", isManuallyOrdered: true });
    const floatingHigh = buildTask({ id: "floating-high", priority: TaskPriority.HIGH, displayRank: "4" });
    const tasks = [pinnedLast, floatingHigh, pinnedFirst];
    const priorityKey = [{ field: "priority" as const, direction: "asc" as const }];

    it("수동 순서 우선이면 직접 배치한 카드가 rank 순으로 위에 남는다", () => {
      // Given / When
      const sorted = sortTasksForBoard(
        tasks,
        preference({ keys: priorityKey, mode: "manual-first" }),
        EMPTY_CONTEXT,
      );

      // Then
      expect(sorted.map((task) => task.id)).toEqual(["pinned-first", "pinned-last", "floating-high"]);
    });

    it("정렬 기준 우선이면 직접 배치한 카드도 정렬 기준을 따른다", () => {
      // Given / When
      const sorted = sortTasksForBoard(
        tasks,
        preference({ keys: priorityKey, mode: "sort-first" }),
        EMPTY_CONTEXT,
      );

      // Then
      expect(sorted.map((task) => task.id)).toEqual(["floating-high", "pinned-first", "pinned-last"]);
    });

    it("수동 순서를 쓰지 않으면 rank는 동점 판정에서도 빠진다", () => {
      // Given
      const olderPinned = buildTask({
        id: "older-pinned",
        priority: TaskPriority.LOW,
        displayRank: "f",
        isManuallyOrdered: true,
        createdAt: new Date("2025-01-01T00:00:00Z"),
      });

      // When
      const sorted = sortTasksForBoard(
        [pinnedFirst, olderPinned],
        preference({ keys: priorityKey, mode: "manual-off" }),
        EMPTY_CONTEXT,
      );

      // Then
      expect(sorted.map((task) => task.id)).toEqual(["older-pinned", "pinned-first"]);
    });
  });
});
