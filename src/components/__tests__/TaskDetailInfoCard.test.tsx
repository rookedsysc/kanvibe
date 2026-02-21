import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import TaskDetailInfoCard from "../TaskDetailInfoCard";
import { TaskStatus } from "@/entities/KanbanTask";
import type { KanbanTask } from "@/entities/KanbanTask";

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string) => {
    const messages: Record<string, string> = {
      info: "작업 정보",
      project: "프로젝트",
      projectColor: "프로젝트 색상",
      priority: "우선순위",
      prLink: "PR 링크",
      agent: "에이전트",
      session: "세션",
      sshHost: "SSH 호스트",
      createdAt: "생성일",
      updatedAt: "수정일",
    };
    return messages[key] ?? key;
  },
}));

vi.mock("@/i18n/navigation", () => ({
  Link: ({ children, href, title, ...props }: Record<string, unknown>) => (
    <a href={href as string} title={title as string} {...props}>{children as React.ReactNode}</a>
  ),
}));

vi.mock("@/components/PriorityEditor", () => ({
  default: () => <div data-testid="priority-editor" />,
}));

vi.mock("@/components/ProjectColorEditor", () => ({
  default: () => <div data-testid="project-color-editor" />,
}));

function createTask(overrides: Partial<KanbanTask> = {}): KanbanTask {
  return {
    id: "task-1",
    title: "Test Task",
    description: null,
    status: TaskStatus.TODO,
    branchName: "feat/test-branch",
    worktreePath: null,
    sessionType: null,
    sessionName: null,
    sshHost: null,
    agentType: null,
    project: {
      id: "project-1",
      name: "kanvibe",
      repoPath: "/path/to/repo",
      defaultBranch: "main",
      sshHost: null,
      isWorktree: false,
      color: null,
      createdAt: new Date(),
    },
    projectId: "project-1",
    baseBranch: "main",
    prUrl: null,
    priority: null,
    displayOrder: 0,
    createdAt: new Date(),
    updatedAt: new Date(),
    ...overrides,
  };
}

describe("TaskDetailInfoCard - Project Shortcut Navigation", () => {
  it("should link to base branch task detail page when baseBranchTaskId is provided", () => {
    // Given
    const task = createTask();
    const baseBranchTaskId = "main-task-123";

    // When
    render(
      <TaskDetailInfoCard
        task={task}
        agentTagStyle={null}
        baseBranchTaskId={baseBranchTaskId}
      />
    );

    // Then
    const shortcutLink = screen.getByTestId("shortcut-link");
    expect(shortcutLink.getAttribute("href")).toBe("/task/main-task-123");
    expect(shortcutLink.getAttribute("title")).toBe("main");
  });

  it("should fallback to board page when baseBranchTaskId is null", () => {
    // Given
    const task = createTask();

    // When
    render(
      <TaskDetailInfoCard
        task={task}
        agentTagStyle={null}
        baseBranchTaskId={null}
      />
    );

    // Then
    const shortcutLink = screen.getByTestId("shortcut-link");
    expect(shortcutLink.getAttribute("href")).toBe("/");
  });

  it("should use project name as title when baseBranch is null", () => {
    // Given
    const task = createTask({ baseBranch: null });

    // When
    render(
      <TaskDetailInfoCard
        task={task}
        agentTagStyle={null}
        baseBranchTaskId={null}
      />
    );

    // Then
    const shortcutLink = screen.getByTestId("shortcut-link");
    expect(shortcutLink.getAttribute("title")).toBe("kanvibe");
    expect(shortcutLink.getAttribute("href")).toBe("/");
  });

  it("should not render shortcut icon when project is null", () => {
    // Given
    const task = createTask({ project: null, projectId: null });

    // When
    render(
      <TaskDetailInfoCard
        task={task}
        agentTagStyle={null}
        baseBranchTaskId={null}
      />
    );

    // Then
    expect(screen.queryByTestId("shortcut-link")).toBeNull();
  });
});

describe("TaskDetailInfoCard - Project Badge Color", () => {
  it("should use fixed bg-tag-project-bg class instead of inline backgroundColor", () => {
    // Given
    const task = createTask({
      project: {
        id: "project-1",
        name: "kanvibe",
        repoPath: "/path/to/repo",
        defaultBranch: "main",
        sshHost: null,
        isWorktree: false,
        color: "#FF0000",
        createdAt: new Date(),
      },
    });

    // When
    render(
      <TaskDetailInfoCard
        task={task}
        agentTagStyle={null}
        baseBranchTaskId={null}
      />
    );

    // Then - 프로젝트 배지가 인라인 스타일 없이 고정 클래스를 사용해야 한다
    const badge = screen.getByText("kanvibe");
    expect(badge.className).toContain("bg-tag-project-bg");
    expect(badge.getAttribute("style")).toBeNull();
  });

  it("should render shortcut link with bg-tag-project-bg class", () => {
    // Given
    const task = createTask();

    // When
    render(
      <TaskDetailInfoCard
        task={task}
        agentTagStyle={null}
        baseBranchTaskId="main-task-123"
      />
    );

    // Then
    const shortcutLink = screen.getByTestId("shortcut-link");
    expect(shortcutLink.className).toContain("bg-tag-project-bg");
  });
});
