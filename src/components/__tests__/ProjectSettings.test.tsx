import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import ProjectSettings from "../ProjectSettings";
import { SessionType } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";
import enMessages from "../../../messages/en.json";
import { deleteProject } from "@/desktop/renderer/actions/project";

const mockSetDefaultSessionType = vi.fn().mockResolvedValue(undefined);
const mockSetNotificationEnabled = vi.fn().mockResolvedValue(undefined);
const mockSetNotificationStatuses = vi.fn().mockResolvedValue(undefined);
const mockSetThemePreference = vi.fn().mockResolvedValue(undefined);

vi.mock("next-intl", () => ({
  useTranslations: (namespace?: string) => (key: string, values?: Record<string, string>) => {
    if (namespace === "settings" && key === "deleteConfirm") {
      return enMessages.settings.deleteConfirm.replace(/\{(\w+)\}/g, (_, name: string) => values?.[name] ?? `{${name}}`);
    }

    return key;
  },
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
  setThemePreference: (...args: unknown[]) => mockSetThemePreference(...args),
  setBackgroundSyncEnabled: vi.fn().mockResolvedValue(undefined),
  setBackgroundSyncIntervalMs: vi.fn().mockResolvedValue(undefined),
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
    delete window.kanvibeDesktop;
    document.documentElement.removeAttribute("data-theme");
    document.documentElement.removeAttribute("data-theme-preference");
    Object.defineProperty(window.navigator, "platform", {
      configurable: true,
      value: "Linux x86_64",
    });
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
        backgroundSyncSettings={{ isEnabled: true, intervalMs: 10 * 60_000 }}
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
        themePreference="system"
        onThemePreferenceChange={onThemePreferenceChange}
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
        backgroundSyncSettings={{ isEnabled: true, intervalMs: 10 * 60_000 }}
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

  it("mac 데스크톱 페이지에서는 Board 링크를 titlebar 버튼 아래로 내린다", async () => {
    window.kanvibeDesktop = { isDesktop: true };
    Object.defineProperty(window.navigator, "platform", {
      configurable: true,
      value: "MacIntel",
    });

    const { container } = render(
      <ProjectSettings
        variant="page"
        projects={[createProject()]}
        sshHosts={[]}
        sidebarDefaultCollapsed={false}
        defaultSessionType={SessionType.TMUX}
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
        backgroundSyncSettings={{ isEnabled: true, intervalMs: 10 * 60_000 }}
      />,
    );

    await waitFor(() => {
      expect(container.querySelector("aside")?.className).toContain("pt-16");
      expect(screen.getByText("Board").closest("a")?.className).toContain("gap-3");
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
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
        backgroundSyncSettings={{ isEnabled: true, intervalMs: 10 * 60_000 }}
      />,
    );

    // When
    fireEvent.keyDown(window, { key: "Escape" });

    // Then
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("프로젝트 삭제 확인 문구는 task DB 삭제와 branch/worktree 보존을 알린다", async () => {
    // Given
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);

    render(
      <ProjectSettings
        isOpen
        onClose={vi.fn()}
        projects={[createProject()]}
        sshHosts={[]}
        sidebarDefaultCollapsed={false}
        defaultSessionType={SessionType.TMUX}
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
        backgroundSyncSettings={{ isEnabled: true, intervalMs: 10 * 60_000 }}
      />,
    );

    try {
      // When
      fireEvent.click(screen.getByRole("button", { name: "deleteProject" }));

      // Then
      expect(confirmSpy).toHaveBeenCalledWith(expect.stringContaining("KanVibe tasks"));
      expect(confirmSpy).toHaveBeenCalledWith(expect.stringContaining("branches and worktrees"));
      await waitFor(() => {
        expect(deleteProject).toHaveBeenCalledWith("project-1");
      });
    } finally {
      confirmSpy.mockRestore();
    }
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
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
        backgroundSyncSettings={{ isEnabled: true, intervalMs: 10 * 60_000 }}
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
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
        backgroundSyncSettings={{ isEnabled: true, intervalMs: 10 * 60_000 }}
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
        notificationSettings={initialSettings}
        backgroundSyncSettings={{ isEnabled: true, intervalMs: 10 * 60_000 }}
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
        notificationSettings={{
          isEnabled: true,
          enabledStatuses: ["progress", "pending", "review"],
        }}
        backgroundSyncSettings={{ isEnabled: true, intervalMs: 10 * 60_000 }}
      />,
    );

    // Then
    await waitFor(() => {
      expect(mockSetNotificationStatuses).toHaveBeenCalledWith(["progress", "review"]);
    });
    expect(screen.getByText("pending").className).toContain("bg-bg-page");
  });

});
