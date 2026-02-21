import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import ProjectSettings from "../ProjectSettings";
import { SessionType } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";

const mockSetDefaultSessionType = vi.fn().mockResolvedValue(undefined);

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string) => key,
}));

vi.mock("@/i18n/navigation", () => ({
  Link: ({
    children,
    href,
    prefetch: _prefetch,
    ...props
  }: {
    children: React.ReactNode;
    href: string;
    prefetch?: boolean;
  }) => (
    <a href={href} {...props}>
      {children}
    </a>
  ),
}));

vi.mock("@/components/FolderSearchInput", () => ({
  default: () => <div data-testid="folder-search-input" />,
}));

vi.mock("@/app/actions/project", () => ({
  deleteProject: vi.fn().mockResolvedValue(undefined),
  scanAndRegisterProjects: vi.fn().mockResolvedValue({ registered: [], skipped: [], errors: [], worktreeTasks: [] }),
}));

vi.mock("@/app/actions/appSettings", () => ({
  setSidebarDefaultCollapsed: vi.fn().mockResolvedValue(undefined),
  setNotificationEnabled: vi.fn().mockResolvedValue(undefined),
  setNotificationStatuses: vi.fn().mockResolvedValue(undefined),
  setDefaultSessionType: (...args: unknown[]) => mockSetDefaultSessionType(...args),
}));

function createProject(): Project {
  return {
    id: "project-1",
    name: "kanvibe",
    repoPath: "/repo/kanvibe",
    defaultBranch: "main",
    sshHost: null,
    isWorktree: false,
    color: null,
    createdAt: new Date(),
  };
}

describe("ProjectSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("기본 세션 타입을 변경하면 onDefaultSessionTypeChange를 호출한다", async () => {
    // Given
    const onDefaultSessionTypeChange = vi.fn();

    render(
      <ProjectSettings
        isOpen
        onClose={vi.fn()}
        projects={[createProject()]}
        sshHosts={[]}
        sidebarDefaultCollapsed={false}
        defaultSessionType={SessionType.TMUX}
        onDefaultSessionTypeChange={onDefaultSessionTypeChange}
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
      />,
    );

    // When
    const sessionTypeSelect = screen.getByDisplayValue(SessionType.TMUX);
    fireEvent.change(sessionTypeSelect, { target: { value: SessionType.ZELLIJ } });

    // Then
    await waitFor(() => {
      expect(mockSetDefaultSessionType).toHaveBeenCalledWith(SessionType.ZELLIJ);
      expect(onDefaultSessionTypeChange).toHaveBeenCalledWith(SessionType.ZELLIJ);
    });
  });
});
