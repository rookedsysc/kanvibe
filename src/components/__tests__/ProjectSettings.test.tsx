import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import ProjectSettings from "../ProjectSettings";
import { SessionType } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";

const mockSetDefaultSessionType = vi.fn().mockResolvedValue(undefined);
const mockSetNotificationEnabled = vi.fn().mockResolvedValue(undefined);
const mockSetNotificationStatuses = vi.fn().mockResolvedValue(undefined);
const mockSetTaskSearchShortcut = vi.fn().mockResolvedValue(undefined);
const mockSetThemePreference = vi.fn().mockResolvedValue(undefined);

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string) => key,
}));

vi.mock("@/desktop/renderer/navigation", () => ({
  Link: ({
    children,
    href,
    prefetch: _prefetch,
    ...props
  }: {
    children: React.ReactNode;
    href: string;
    prefetch?: boolean;
  }) => {
    void _prefetch;

    return (
      <a href={href} {...props}>
        {children}
      </a>
    );
  },
}));

vi.mock("@/components/FolderSearchInput", () => ({
  default: () => <div data-testid="folder-search-input" />,
}));

vi.mock("@/desktop/renderer/actions/project", () => ({
  deleteProject: vi.fn().mockResolvedValue(undefined),
  scanAndRegisterProjects: vi.fn().mockResolvedValue({ registered: [], skipped: [], errors: [], worktreeTasks: [] }),
}));

vi.mock("@/desktop/renderer/actions/appSettings", () => ({
  setSidebarDefaultCollapsed: vi.fn().mockResolvedValue(undefined),
  setNotificationEnabled: (...args: unknown[]) => mockSetNotificationEnabled(...args),
  setNotificationStatuses: (...args: unknown[]) => mockSetNotificationStatuses(...args),
  setDefaultSessionType: (...args: unknown[]) => mockSetDefaultSessionType(...args),
  setTaskSearchShortcut: (...args: unknown[]) => mockSetTaskSearchShortcut(...args),
  setThemePreference: (...args: unknown[]) => mockSetThemePreference(...args),
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
    document.documentElement.removeAttribute("data-theme");
    document.documentElement.removeAttribute("data-theme-preference");
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
        taskSearchShortcut="Mod+Shift+O"
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

  it("테마 설정을 변경하면 즉시 DOM 테마와 저장 값을 갱신한다", async () => {
    const onThemePreferenceChange = vi.fn();

    render(
      <ProjectSettings
        isOpen
        onClose={vi.fn()}
        projects={[createProject()]}
        sshHosts={[]}
        sidebarDefaultCollapsed={false}
        defaultSessionType={SessionType.TMUX}
        taskSearchShortcut="Mod+Shift+O"
        themePreference="system"
        onThemePreferenceChange={onThemePreferenceChange}
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "theme.dark" }));

    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(document.documentElement.dataset.themePreference).toBe("dark");
    expect(onThemePreferenceChange).toHaveBeenCalledWith("dark");
    await waitFor(() => {
      expect(mockSetThemePreference).toHaveBeenCalledWith("dark");
    });
  });

  it("Escape를 누르면 설정 패널을 닫는다", () => {
    // Given
    const onClose = vi.fn();

    render(
      <ProjectSettings
        isOpen
        onClose={onClose}
        projects={[createProject()]}
        sshHosts={[]}
        sidebarDefaultCollapsed={false}
        defaultSessionType={SessionType.TMUX}
        taskSearchShortcut="Mod+Shift+O"
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
      />,
    );

    // When
    fireEvent.keyDown(window, { key: "Escape" });

    // Then
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("알림 활성화 토글은 로컬 상태를 즉시 반영한다", async () => {
    // Given
    render(
      <ProjectSettings
        isOpen
        onClose={vi.fn()}
        projects={[createProject()]}
        sshHosts={[]}
        sidebarDefaultCollapsed={false}
        defaultSessionType={SessionType.TMUX}
        taskSearchShortcut="Mod+Shift+O"
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
      />,
    );

    const switches = screen.getAllByRole("switch");

    // When
    fireEvent.click(switches[1]);

    // Then
    expect(switches[1].getAttribute("aria-checked")).toBe("false");
    await waitFor(() => {
      expect(mockSetNotificationEnabled).toHaveBeenCalledWith(false);
    });
  });

  it("알림 상태 선택 버튼은 클릭 즉시 저장을 호출한다", async () => {
    // Given
    render(
      <ProjectSettings
        isOpen
        onClose={vi.fn()}
        projects={[createProject()]}
        sshHosts={[]}
        sidebarDefaultCollapsed={false}
        defaultSessionType={SessionType.TMUX}
        taskSearchShortcut="Mod+Shift+O"
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
      />,
    );

    // When
    fireEvent.click(screen.getByText("pending"));

    // Then
    await waitFor(() => {
      expect(mockSetNotificationStatuses).toHaveBeenCalledWith(["progress", "review"]);
    });
  });

  it("stale props가 다시 들어와도 방금 바꾼 알림 상태를 덮어쓰지 않는다", async () => {
    // Given
    const initialSettings = {
      isEnabled: true,
      enabledStatuses: ["progress", "pending", "review"],
    };

    const { rerender } = render(
      <ProjectSettings
        isOpen
        onClose={vi.fn()}
        projects={[createProject()]}
        sshHosts={[]}
        sidebarDefaultCollapsed={false}
        defaultSessionType={SessionType.TMUX}
        taskSearchShortcut="Mod+Shift+O"
        notificationSettings={initialSettings}
      />,
    );

    // When
    fireEvent.click(screen.getByText("pending"));
    rerender(
      <ProjectSettings
        isOpen
        onClose={vi.fn()}
        projects={[createProject()]}
        sshHosts={[]}
        sidebarDefaultCollapsed={false}
        defaultSessionType={SessionType.TMUX}
        taskSearchShortcut="Mod+Shift+O"
        notificationSettings={{
          isEnabled: true,
          enabledStatuses: ["progress", "pending", "review"],
        }}
      />,
    );

    // Then
    await waitFor(() => {
      expect(mockSetNotificationStatuses).toHaveBeenCalledWith(["progress", "review"]);
    });
    expect(screen.getByText("pending").className).toContain("bg-bg-page");
  });

  it("검색 단축키를 캡처해서 즉시 저장한다", async () => {
    // Given
    render(
      <ProjectSettings
        isOpen
        onClose={vi.fn()}
        projects={[createProject()]}
        sshHosts={[]}
        sidebarDefaultCollapsed={false}
        defaultSessionType={SessionType.TMUX}
        taskSearchShortcut="Mod+Shift+O"
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
      />,
    );

    const recordButton = screen.getByTestId("task-search-shortcut-record");

    // When
    fireEvent.click(recordButton);
    fireEvent.keyDown(recordButton, {
      key: "p",
      ctrlKey: true,
      shiftKey: true,
    });

    // Then
    await waitFor(() => {
      expect(mockSetTaskSearchShortcut).toHaveBeenCalledWith("Mod+Shift+P");
    });
    expect(screen.getByText("Ctrl+Shift+P")).toBeTruthy();
  });

  it("Cmd/Ctrl+R은 검색 단축키로 저장하지 않는다", () => {
    render(
      <ProjectSettings
        isOpen
        onClose={vi.fn()}
        projects={[createProject()]}
        sshHosts={[]}
        sidebarDefaultCollapsed={false}
        defaultSessionType={SessionType.TMUX}
        taskSearchShortcut="Mod+Shift+O"
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
      />,
    );

    const recordButton = screen.getByTestId("task-search-shortcut-record");

    fireEvent.click(recordButton);
    fireEvent.keyDown(recordButton, {
      key: "r",
      ctrlKey: true,
    });

    expect(mockSetTaskSearchShortcut).not.toHaveBeenCalled();
    expect(screen.getByText("taskSearchShortcutRecording")).toBeTruthy();
  });
});
