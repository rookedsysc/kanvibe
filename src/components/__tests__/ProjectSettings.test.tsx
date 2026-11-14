import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ProjectSettings from "../ProjectSettings";
import { SessionType } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";
import enMessages from "../../../messages/en.json";

const mockSetDefaultSessionType = vi.fn().mockResolvedValue(undefined);
const mockSetNotificationEnabled = vi.fn().mockResolvedValue(undefined);
const mockSetNotificationStatuses = vi.fn().mockResolvedValue(undefined);
const mockSetThemePreference = vi.fn().mockResolvedValue(undefined);
const mockSetVimModeEnabled = vi.fn().mockResolvedValue(undefined);
const mockSetBackgroundSyncEnabled = vi.fn().mockResolvedValue(undefined);
const mockSetBackgroundSyncIntervalMs = vi.fn().mockResolvedValue(undefined);

vi.mock("next-intl", () => ({
  useTranslations: (namespace?: string) => (key: string, values?: Record<string, string>) => {
    if (namespace === "settings" && key === "deleteConfirm") {
      return enMessages.settings.deleteConfirm.replace(/\{(\w+)\}/g, (_, name: string) => values?.[name] ?? `{${name}}`);
    }

    return values ? `${key}:${JSON.stringify(values)}` : key;
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

vi.mock("@/components/BulkSessionTypeChangeSettings", () => ({
  default: () => <div data-testid="bulk-session-type-change-settings" />,
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
  setVimModeEnabled: (...args: unknown[]) => mockSetVimModeEnabled(...args),
  setBackgroundSyncEnabled: (...args: unknown[]) => mockSetBackgroundSyncEnabled(...args),
  setBackgroundSyncIntervalMs: (...args: unknown[]) => mockSetBackgroundSyncIntervalMs(...args),
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
    iconDataUrl: null,
    createdAt: new Date(),
  };
}

/** 같은 화면의 다른 영역도 오류 알림을 띄우므로, 설정 저장 실패 알림만 문구로 골라낸다 */
async function findSettingSaveFailure(message: string) {
  return waitFor(() => {
    const saveFailure = screen.getAllByRole("alert").find((alert) => alert.textContent?.includes(message));
    expect(saveFailure).toBeTruthy();
    return saveFailure as HTMLElement;
  });
}

describe("ProjectSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockSetBackgroundSyncEnabled.mockResolvedValue(undefined);
    mockSetBackgroundSyncIntervalMs.mockResolvedValue(undefined);
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

  it("vim mode 토글을 변경하면 로컬 상태와 저장 값을 갱신한다", async () => {
    const onVimModeEnabledChange = vi.fn();

    render(
      <ProjectSettings
        isOpen
        onClose={vi.fn()}
        projects={[createProject()]}
        sshHosts={[]}
        sidebarDefaultCollapsed={false}
        defaultSessionType={SessionType.TMUX}
        vimModeEnabled
        onVimModeEnabledChange={onVimModeEnabledChange}
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
        backgroundSyncSettings={{ isEnabled: true, intervalMs: 10 * 60_000 }}
      />,
    );

    const vimModeSwitch = screen.getByRole("switch", { name: "vimModeEnabled" });

    fireEvent.click(vimModeSwitch);

    expect(vimModeSwitch.getAttribute("aria-checked")).toBe("false");
    expect(onVimModeEnabledChange).toHaveBeenCalledWith(false);
    await waitFor(() => {
      expect(mockSetVimModeEnabled).toHaveBeenCalledWith(false);
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

  it("설정 화면에서는 프로젝트 스캔과 등록 프로젝트 목록을 렌더링하지 않는다", () => {
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

    expect(screen.queryByTestId("folder-search-input")).toBeNull();
    expect(screen.queryByRole("button", { name: "deleteProject" })).toBeNull();
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

  it("background sync 주기 입력은 기존 한 자리 값을 지운 뒤 두 자리 분 값을 입력할 수 있다", async () => {
    // Given
    const user = userEvent.setup();

    render(
      <ProjectSettings
        isOpen
        onClose={vi.fn()}
        projects={[createProject()]}
        sshHosts={[]}
        sidebarDefaultCollapsed={false}
        defaultSessionType={SessionType.TMUX}
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
        backgroundSyncSettings={{ isEnabled: true, intervalMs: 1 * 60_000 }}
      />,
    );

    const intervalInput = screen.getByRole("spinbutton") as HTMLInputElement;

    // When
    await user.clear(intervalInput);
    await user.type(intervalInput, "10");

    // Then
    expect(intervalInput.value).toBe("10");
    await waitFor(() => {
      expect(mockSetBackgroundSyncIntervalMs).toHaveBeenLastCalledWith(10 * 60_000);
    });
  });

  it("background sync 주기 저장은 이전 저장이 끝난 뒤 최신 값을 저장한다", async () => {
    // Given
    const user = userEvent.setup();
    let resolveFirstSave: () => void = () => {};
    mockSetBackgroundSyncIntervalMs
      .mockImplementationOnce(() => new Promise<void>((resolve) => {
        resolveFirstSave = resolve;
      }))
      .mockResolvedValue(undefined);

    render(
      <ProjectSettings
        isOpen
        onClose={vi.fn()}
        projects={[createProject()]}
        sshHosts={[]}
        sidebarDefaultCollapsed={false}
        defaultSessionType={SessionType.TMUX}
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
        backgroundSyncSettings={{ isEnabled: true, intervalMs: 1 * 60_000 }}
      />,
    );

    const intervalInput = screen.getByRole("spinbutton") as HTMLInputElement;

    // When
    await user.clear(intervalInput);
    await user.type(intervalInput, "20");

    // Then
    expect(intervalInput.value).toBe("20");
    expect(mockSetBackgroundSyncIntervalMs).toHaveBeenCalledTimes(1);
    expect(mockSetBackgroundSyncIntervalMs).toHaveBeenCalledWith(2 * 60_000);

    resolveFirstSave();

    await waitFor(() => {
      expect(mockSetBackgroundSyncIntervalMs).toHaveBeenCalledTimes(2);
    });
    expect(mockSetBackgroundSyncIntervalMs).toHaveBeenLastCalledWith(20 * 60_000);
  });

  it("vim mode 저장이 실패하면 저장된 값으로 되돌리고 사유와 다시 시도를 보인다", async () => {
    const onVimModeEnabledChange = vi.fn();
    mockSetVimModeEnabled
      .mockRejectedValueOnce(new Error("settings db locked"))
      .mockResolvedValueOnce(undefined);

    render(
      <ProjectSettings
        isOpen
        onClose={vi.fn()}
        projects={[createProject()]}
        sshHosts={[]}
        sidebarDefaultCollapsed={false}
        defaultSessionType={SessionType.TMUX}
        vimModeEnabled
        onVimModeEnabledChange={onVimModeEnabledChange}
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
        backgroundSyncSettings={{ isEnabled: true, intervalMs: 10 * 60_000 }}
      />,
    );

    const vimModeSwitch = screen.getByRole("switch", { name: "vimModeEnabled" });
    fireEvent.click(vimModeSwitch);

    const saveFailure = await findSettingSaveFailure('settingSaveFailed:{"reason":"settings db locked"}');
    expect(vimModeSwitch.getAttribute("aria-checked")).toBe("true");
    expect(onVimModeEnabledChange).toHaveBeenLastCalledWith(true);

    fireEvent.click(within(saveFailure).getByRole("button", { name: "retry" }));

    await waitFor(() => {
      expect(vimModeSwitch.getAttribute("aria-checked")).toBe("false");
    });
    expect(mockSetVimModeEnabled).toHaveBeenCalledTimes(2);
    await waitFor(() => {
      expect(screen.queryByText(/settingSaveFailed/)).toBeNull();
    });
  });

  it("한 설정을 저장하는 동안 그 설정만 막고 다른 설정은 바꿀 수 있다", async () => {
    let resolveVimSave: () => void = () => {};
    mockSetVimModeEnabled.mockReturnValueOnce(new Promise<void>((resolve) => {
      resolveVimSave = resolve;
    }));

    render(
      <ProjectSettings
        isOpen
        onClose={vi.fn()}
        projects={[createProject()]}
        sshHosts={[]}
        sidebarDefaultCollapsed={false}
        defaultSessionType={SessionType.TMUX}
        vimModeEnabled
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
        backgroundSyncSettings={{ isEnabled: true, intervalMs: 10 * 60_000 }}
      />,
    );

    const vimModeSwitch = screen.getByRole("switch", { name: "vimModeEnabled" });
    fireEvent.click(vimModeSwitch);

    expect((vimModeSwitch as HTMLButtonElement).disabled).toBe(true);
    const sidebarSwitch = screen.getAllByRole("switch")[0];
    expect((sidebarSwitch as HTMLButtonElement).disabled).toBe(false);

    resolveVimSave();
    await waitFor(() => {
      expect((vimModeSwitch as HTMLButtonElement).disabled).toBe(false);
    });
  });

  it("알림 설정 저장이 실패하면 이전 값으로 되돌린다", async () => {
    mockSetNotificationEnabled.mockRejectedValueOnce(new Error("ipc closed"));

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

    const notificationSwitch = screen.getAllByRole("switch")[1];
    fireEvent.click(notificationSwitch);

    await findSettingSaveFailure('settingSaveFailed:{"reason":"ipc closed"}');
    expect(notificationSwitch.getAttribute("aria-checked")).toBe("true");
  });
});
