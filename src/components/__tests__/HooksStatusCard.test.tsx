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
  mockGetTaskHooksStatus,
  mockGetTaskGeminiHooksStatus,
  mockGetTaskCodexHooksStatus,
  mockGetTaskOpenCodeHooksStatus,
} = vi.hoisted(() => ({
  mockInstallTaskHooks: vi.fn(),
  mockInstallTaskGeminiHooks: vi.fn(),
  mockInstallTaskCodexHooks: vi.fn(),
  mockInstallTaskOpenCodeHooks: vi.fn(),
  mockGetTaskHooksStatus: vi.fn(),
  mockGetTaskGeminiHooksStatus: vi.fn(),
  mockGetTaskCodexHooksStatus: vi.fn(),
  mockGetTaskOpenCodeHooksStatus: vi.fn(),
}));

vi.mock("@/desktop/renderer/actions/project", () => ({
  installTaskHooks: mockInstallTaskHooks,
  installTaskGeminiHooks: mockInstallTaskGeminiHooks,
  installTaskCodexHooks: mockInstallTaskCodexHooks,
  installTaskOpenCodeHooks: mockInstallTaskOpenCodeHooks,
  getTaskHooksStatus: mockGetTaskHooksStatus,
  getTaskGeminiHooksStatus: mockGetTaskGeminiHooksStatus,
  getTaskCodexHooksStatus: mockGetTaskCodexHooksStatus,
  getTaskOpenCodeHooksStatus: mockGetTaskOpenCodeHooksStatus,
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
    hooksInstallIncomplete: "Hooks were installed, but verification is incomplete.",
    hooksInstallFailed: "Hooks installation failed",
    hooksRemoteNotSupported: "Remote projects are not supported",
    hooksCurrentTaskId: "Current taskId: {taskId}",
    hooksStatusDialog: {
      reinstall: "Reinstall",
    },
  },
};

describe("HooksStatusCard", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetTaskHooksStatus.mockResolvedValue(null);
    mockGetTaskGeminiHooksStatus.mockResolvedValue(null);
    mockGetTaskCodexHooksStatus.mockResolvedValue(null);
    mockGetTaskOpenCodeHooksStatus.mockResolvedValue(null);
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

  it("uses dedicated tool icons for hook status rows", () => {
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
    expect(toolIcons.map((icon) => icon.getAttribute("data-icon-name"))).toEqual([
      "ClaudeLogoIcon",
      "GeminiLogoIcon",
      "CodexLogoIcon",
      "OpenCodeLogoIcon",
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

  it("installs Claude hooks from the inline action and updates local status", async () => {
    const installedClaudeStatus = { installed: true, hasPromptHook: true, hasStopHook: true, hasQuestionHook: true, hasSettingsEntry: true };
    const installedGeminiStatus = { installed: true, hasPromptHook: true, hasStopHook: true, hasSettingsEntry: true };
    const installedCodexStatus = { installed: true, hasPromptHook: true, hasPermissionHook: true, hasPreToolHook: true, hasStopHook: true, hasHooksFile: true, hasHookEntries: true, hasConfigEntry: true };
    const installedOpenCodeStatus = { installed: true, hasPlugin: true, hasTaskIdBinding: true, hasStatusEndpoint: true, hasEventMappings: true, hasMainSessionGuard: true, hasDuplicateProgressGuard: true };
    mockInstallTaskHooks.mockResolvedValue({
      success: true,
      status: installedClaudeStatus,
    });
    mockGetTaskHooksStatus.mockResolvedValue(installedClaudeStatus);
    mockGetTaskGeminiHooksStatus.mockResolvedValue(installedGeminiStatus);
    mockGetTaskCodexHooksStatus.mockResolvedValue(installedCodexStatus);
    mockGetTaskOpenCodeHooksStatus.mockResolvedValue(installedOpenCodeStatus);
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
  });
});
