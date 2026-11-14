import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import SettingsRoute from "@/desktop/renderer/routes/SettingsRoute";
import { INITIAL_DESKTOP_LOAD_TIMEOUT_MS } from "@/desktop/renderer/utils/loadingTimeout";

const mocks = vi.hoisted(() => ({
  getAllProjects: vi.fn(),
  refreshSignal: 0,
}));

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, unknown>) => (
    values ? `${key}:${JSON.stringify(values)}` : key
  ),
}));

vi.mock("@/components/ProjectSettings", () => ({
  default: ({ projects, themePreference }: { projects: Array<{ name: string }>; themePreference: string }) => (
    <div data-testid="project-settings">{`${projects.map((project) => project.name).join(",")}|${themePreference}`}</div>
  ),
}));

vi.mock("@/desktop/renderer/actions/project", () => ({
  getAllProjects: (...args: unknown[]) => mocks.getAllProjects(...args),
  getAvailableHosts: vi.fn().mockResolvedValue([]),
}));

vi.mock("@/desktop/renderer/actions/appSettings", () => ({
  getBackgroundSyncSettings: vi.fn().mockResolvedValue({ isEnabled: false, intervalMs: 600_000 }),
  getDefaultSessionType: vi.fn().mockResolvedValue("tmux"),
  getNotificationSettings: vi.fn().mockResolvedValue({ isEnabled: true, enabledStatuses: [] }),
  getSidebarDefaultCollapsed: vi.fn().mockResolvedValue(false),
  getThemePreference: vi.fn().mockResolvedValue("dark"),
  getVimModeEnabled: vi.fn().mockResolvedValue(true),
}));

vi.mock("@/desktop/renderer/navigation", () => ({
  useRouter: () => ({ back: vi.fn() }),
}));

vi.mock("@/desktop/renderer/utils/refresh", () => ({
  useRefreshSignal: () => mocks.refreshSignal,
}));

describe("SettingsRoute", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getAllProjects.mockReset();
    mocks.refreshSignal = 0;
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("설정 조회가 오래 걸리면 기본 설정을 채우지 않고 지연 안내를 띄운 채 기다린다", async () => {
    // Given
    vi.useFakeTimers();
    mocks.getAllProjects.mockReturnValue(new Promise(() => {}));

    // When
    render(<SettingsRoute />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(INITIAL_DESKTOP_LOAD_TIMEOUT_MS);
    });

    // Then
    expect(screen.getByRole("status").textContent).toBe("loadDelayed");
    expect(screen.queryByTestId("project-settings")).toBeNull();
  });

  it("설정 조회가 실패하면 기본 설정 대신 실패 사유와 재시도를 보이고 재시도로 다시 불러온다", async () => {
    // Given
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.getAllProjects
      .mockRejectedValueOnce(new Error("settings db locked"))
      .mockResolvedValueOnce([{ name: "kanvibe" }]);

    try {
      // When
      render(<SettingsRoute />);

      // Then
      const alert = await screen.findByRole("alert");
      expect(alert.textContent).toContain('loadFailed:{"reason":"settings db locked"}');
      expect(screen.queryByTestId("project-settings")).toBeNull();

      fireEvent.click(screen.getByRole("button", { name: "retry" }));

      await waitFor(() => {
        expect(screen.getByTestId("project-settings").textContent).toBe("kanvibe|dark");
      });
    } finally {
      consoleError.mockRestore();
    }
  });

  it("이미 불러온 설정을 새로 고치다 실패하면 기존 설정을 유지하고 갱신 실패 배너를 보인다", async () => {
    // Given
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.getAllProjects
      .mockResolvedValueOnce([{ name: "kanvibe" }])
      .mockRejectedValueOnce(new Error("ipc closed"));
    const { rerender } = render(<SettingsRoute />);
    await screen.findByTestId("project-settings");

    try {
      // When
      mocks.refreshSignal = 1;
      rerender(<SettingsRoute />);

      // Then
      const alert = await screen.findByRole("alert");
      expect(alert.textContent).toContain('refreshFailed:{"reason":"ipc closed"}');
      expect(screen.getByTestId("project-settings").textContent).toBe("kanvibe|dark");
    } finally {
      consoleError.mockRestore();
    }
  });
});
