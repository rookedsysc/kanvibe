import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import type { ComponentProps } from "react";
import { IntlProvider } from "next-intl";
import HooksStatusCard from "@/components/HooksStatusCard";

const {
  mockInstallTaskHooks,
  mockInstallTaskGeminiHooks,
  mockInstallTaskCodexHooks,
  mockInstallTaskOpenCodeHooks,
  mockGetTaskHooksStatuses,
} = vi.hoisted(() => ({
  mockInstallTaskHooks: vi.fn(),
  mockInstallTaskGeminiHooks: vi.fn(),
  mockInstallTaskCodexHooks: vi.fn(),
  mockInstallTaskOpenCodeHooks: vi.fn(),
  mockGetTaskHooksStatuses: vi.fn(),
}));

vi.mock("@/desktop/renderer/actions/project", () => ({
  installTaskHooks: mockInstallTaskHooks,
  installTaskGeminiHooks: mockInstallTaskGeminiHooks,
  installTaskCodexHooks: mockInstallTaskCodexHooks,
  installTaskOpenCodeHooks: mockInstallTaskOpenCodeHooks,
  getTaskHooksStatuses: mockGetTaskHooksStatuses,
}));

vi.mock("@hugeicons/react", () => ({
  HugeiconsIcon: ({
    icon,
    "data-testid": testId = "hugeicons-icon",
    ...props
  }: {
    icon?: { __iconName?: string };
    "data-testid"?: string;
  }) => (
    <svg
      {...props}
      data-testid={testId}
      data-icon-name={icon?.__iconName ?? "unknown"}
    />
  ),
}));

vi.mock("@hugeicons/core-free-icons", () => ({
  AlertCircleIcon: { __iconName: "AlertCircleIcon" },
  CheckmarkCircle02Icon: { __iconName: "CheckmarkCircle02Icon" },
  Clock01Icon: { __iconName: "Clock01Icon" },
}));

const messages = {
  taskDetail: {
    hooksStatus: "Hooks Status",
    hooksInstalled: "Installed",
    hooksNotInstalled: "Not Installed",
    installHooks: "Install Hooks",
    installingHooks: "Installing...",
    hooksInstallSuccess: "Hooks installed successfully",
    geminiHooksInstallSuccess: "Gemini CLI hooks installed",
    codexHooksInstallSuccess: "Codex CLI hooks installed",
    openCodeHooksInstallSuccess: "OpenCode hooks installed",
    hooksVerificationFailed: "Verification failed",
    hooksInstallIncomplete: "Hooks were installed, but verification is incomplete.",
    hooksInstallFailed: "Hooks installation failed",
    hooksRemoteNotSupported: "Remote projects are not supported",
    hooksCurrentTaskId: "Current taskId: {taskId}",
    hooksStatusDialog: {
      reinstall: "Reinstall",
    },
  },
  common: {
    retry: "Retry",
    loading: "Loading...",
  },
};

describe("HooksStatusCard", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetTaskHooksStatuses.mockResolvedValue(null);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  const renderCard = (props: ComponentProps<typeof HooksStatusCard>) => {
    return render(
      <IntlProvider messages={messages} locale="en">
        <HooksStatusCard {...props} />
      </IntlProvider>
    );
  };

  it("renders hook tools inline without opening another dialog", () => {
    renderCard({
      taskId: "task-1",
      initialClaudeStatus: null,
      initialGeminiStatus: null,
      initialCodexStatus: null,
      initialOpenCodeStatus: null,
      isRemote: false,
    });

    expect(screen.getByText("Hooks Status")).toBeTruthy();
    expect(screen.getByText("Claude")).toBeTruthy();
    expect(screen.getByText("Gemini")).toBeTruthy();
    expect(screen.getByText("Codex")).toBeTruthy();
    expect(screen.getByText("OpenCode")).toBeTruthy();
    expect(screen.queryByTestId("hooks-status-dialog")).toBeNull();
  });

  it("uses LobeHub provider icons for hook status rows", () => {
    renderCard({
      taskId: "task-1",
      initialClaudeStatus: null,
      initialGeminiStatus: null,
      initialCodexStatus: null,
      initialOpenCodeStatus: null,
      isRemote: false,
    });

    const toolIcons = screen.getAllByTestId("hook-status-tool-icon");

    expect(toolIcons).toHaveLength(4);
    expect(toolIcons.map((icon) => icon.getAttribute("data-icon-source"))).toEqual([
      "lobehub-icons",
      "lobehub-icons",
      "lobehub-icons",
      "lobehub-icons",
    ]);
    expect(toolIcons.map((icon) => icon.getAttribute("data-icon-name"))).toEqual([
      "Claude",
      "Gemini",
      "Codex",
      "OpenCode",
    ]);
    expect(screen.getByTestId("hooks-overall-status-icon").getAttribute("data-icon-name")).toBe("AlertCircleIcon");
  });

  it("shows reinstall action immediately when a hook is already installed", () => {
    renderCard({
      taskId: "task-1",
      initialClaudeStatus: { installed: true, hasPromptHook: true, hasStopHook: true, hasQuestionHook: true, hasSettingsEntry: true },
      initialGeminiStatus: null,
      initialCodexStatus: null,
      initialOpenCodeStatus: null,
      isRemote: false,
    });

    expect(screen.getByRole("button", { name: "Reinstall Claude" })).toBeTruthy();
  });

  it("does not describe remote hook status as unsupported", () => {
    renderCard({
      taskId: "task-remote",
      initialClaudeStatus: null,
      initialGeminiStatus: null,
      initialCodexStatus: null,
      initialOpenCodeStatus: null,
      isRemote: true,
    });

    expect(screen.queryByText("Remote projects are not supported")).toBeNull();
  });

  it("reuses the parent OpenCode status without a mount request", async () => {
    renderCard({
      taskId: "task-1",
      initialClaudeStatus: null,
      initialGeminiStatus: null,
      initialCodexStatus: null,
      initialOpenCodeStatus: { installed: true, hasPlugin: true },
      isRemote: false,
    });
    await waitFor(() => expect(screen.getByRole("button", { name: "Reinstall OpenCode" })).toBeTruthy());
    expect(mockGetTaskHooksStatuses).not.toHaveBeenCalled();
  });

  it("distinguishes failed OpenCode verification from missing hooks", () => {
    renderCard({
      taskId: "task-1",
      initialClaudeStatus: null,
      initialGeminiStatus: null,
      initialCodexStatus: null,
      initialOpenCodeStatus: { installed: false, hasPlugin: true, pluginRegistrationError: "timeout" },
      isRemote: false,
    });
    expect(screen.getByText("Verification failed")).toBeTruthy();
    expect(mockGetTaskHooksStatuses).not.toHaveBeenCalled();
  });

  it("installs Claude hooks from the inline action and updates local status", async () => {
    const installedClaudeStatus = { installed: true, hasPromptHook: true, hasStopHook: true, hasQuestionHook: true, hasSettingsEntry: true };
    const installedGeminiStatus = { installed: true, hasPromptHook: true, hasStopHook: true, hasSettingsEntry: true };
    const installedCodexStatus = { installed: true, hasPromptHook: true, hasPermissionHook: true, hasPreToolHook: true, hasStopHook: true, hasHooksFile: true, hasHookEntries: true, hasConfigEntry: true };
    const installedOpenCodeStatus = { installed: true, hasPlugin: true, hasTaskIdBinding: true, hasStatusEndpoint: true, hasEventMappings: true, hasMainSessionGuard: true, hasDuplicateProgressGuard: true };
    mockInstallTaskHooks.mockResolvedValue({
      success: true,
      status: installedClaudeStatus,
    });
    mockGetTaskHooksStatuses.mockResolvedValue({
      claude: installedClaudeStatus,
      gemini: installedGeminiStatus,
      codex: installedCodexStatus,
      openCode: installedOpenCodeStatus,
    });
    const onStatusesChange = vi.fn();

    renderCard({
      taskId: "task-1",
      initialClaudeStatus: null,
      initialGeminiStatus: null,
      initialCodexStatus: null,
      initialOpenCodeStatus: null,
      isRemote: false,
      onStatusesChange,
    });

    fireEvent.click(screen.getByRole("button", { name: "Install Hooks Claude" }));

    await waitFor(() => {
      expect(mockInstallTaskHooks).toHaveBeenCalledWith("task-1");
      expect(onStatusesChange).toHaveBeenCalledWith({
        claudeStatus: installedClaudeStatus,
        geminiStatus: installedGeminiStatus,
        codexStatus: installedCodexStatus,
        openCodeStatus: installedOpenCodeStatus,
      });
    });
    expect(screen.getByText("Hooks installed successfully")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Reinstall Claude" })).toBeTruthy();
    // 원격 task에서 provider마다 SSH 접속이 생기지 않도록 설치 뒤 새로고침은 한 번의 조회로 끝낸다.
    expect(mockGetTaskHooksStatuses).toHaveBeenCalledTimes(1);
    expect(mockGetTaskHooksStatuses).toHaveBeenCalledWith("task-1");
  });

  it("상태를 읽는 동안에는 읽는 중임을 알리고, 다 읽으면 지운다", () => {
    const pendingProps = {
      taskId: "task-1",
      initialClaudeStatus: null,
      initialGeminiStatus: null,
      initialCodexStatus: null,
      initialOpenCodeStatus: null,
      isRemote: true,
    };
    const { rerender } = renderCard({ ...pendingProps, isLoading: true });

    expect(screen.getByRole("status").textContent).toBe("Loading...");

    rerender(
      <IntlProvider messages={messages} locale="en">
        <HooksStatusCard {...pendingProps} isLoading={false} />
      </IntlProvider>
    );

    expect(screen.queryByRole("status")).toBeNull();
  });
  it("설치 요청이 결과 없이 실패해도 사유와 다시 시도를 보이고 다시 시도하면 같은 도구를 설치한다", async () => {
    mockInstallTaskHooks
      .mockRejectedValueOnce(new Error("ipc closed"))
      .mockResolvedValueOnce({ success: true, status: { installed: true } });
    renderCard({
      taskId: "task-1",
      initialClaudeStatus: null,
      initialGeminiStatus: null,
      initialCodexStatus: null,
      initialOpenCodeStatus: null,
      isRemote: false,
    });

    fireEvent.click(screen.getByRole("button", { name: "Install Hooks Claude" }));

    const failure = await screen.findByRole("alert");
    expect(failure.textContent).toContain("Hooks installation failed");

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    await waitFor(() => {
      expect(mockInstallTaskHooks).toHaveBeenCalledTimes(2);
    });
    await waitFor(() => {
      expect(screen.queryByRole("alert")).toBeNull();
    });
  });
});
