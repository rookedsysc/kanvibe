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
  mockGetTaskOpenCodeHooksStatus,
} = vi.hoisted(() => ({
  mockInstallTaskHooks: vi.fn(),
  mockInstallTaskGeminiHooks: vi.fn(),
  mockInstallTaskCodexHooks: vi.fn(),
  mockInstallTaskOpenCodeHooks: vi.fn(),
  mockGetTaskOpenCodeHooksStatus: vi.fn(),
}));

vi.mock("@/desktop/renderer/actions/project", () => ({
  installTaskHooks: mockInstallTaskHooks,
  installTaskGeminiHooks: mockInstallTaskGeminiHooks,
  installTaskCodexHooks: mockInstallTaskCodexHooks,
  installTaskOpenCodeHooks: mockInstallTaskOpenCodeHooks,
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
    hooksCurrentTaskId: "Current taskId: {taskId}",
    hooksStatusDialog: {
      reinstall: "Reinstall",
    },
  },
};

describe("HooksStatusCard", () => {
  beforeEach(() => {
    vi.clearAllMocks();
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

  it("installs Claude hooks from the inline action and updates local status", async () => {
    mockInstallTaskHooks.mockResolvedValue({
      success: true,
      status: { installed: true, hasPromptHook: true, hasStopHook: true, hasQuestionHook: true, hasSettingsEntry: true },
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
        claudeStatus: { installed: true, hasPromptHook: true, hasStopHook: true, hasQuestionHook: true, hasSettingsEntry: true },
      });
    });
    expect(screen.getByText("Hooks installed successfully")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Reinstall Claude" })).toBeTruthy();
  });
});
