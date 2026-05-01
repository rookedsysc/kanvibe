import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import type { ComponentProps } from "react";
import HooksStatusDialog from "@/components/HooksStatusDialog";

// --- Mocks ---

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

/** useTranslations mock은 key를 그대로 반환한다 */
vi.mock("next-intl", async () => {
  const actual = await vi.importActual("next-intl");
  return {
    ...actual,
    useTranslations: () => (key: string) => key,
  };
});

describe("HooksStatusDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetTaskOpenCodeHooksStatus.mockResolvedValue(null);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  const renderDialog = (props: ComponentProps<typeof HooksStatusDialog>) => {
    return render(<HooksStatusDialog {...props} />);
  };

  const verifiedClaudeStatus = {
    installed: true,
    hasPromptHook: true,
    hasStopHook: true,
    hasQuestionHook: true,
    hasSettingsEntry: true,
    hasTaskIdBinding: true,
    hasStatusMappings: true,
  };

  const verifiedGeminiStatus = {
    installed: true,
    hasPromptHook: true,
    hasStopHook: true,
    hasSettingsEntry: true,
    hasTaskIdBinding: true,
    hasStatusMappings: true,
  };

  const verifiedCodexStatus = {
    installed: true,
    hasPromptHook: true,
    hasPermissionHook: true,
    hasPreToolHook: true,
    hasStopHook: true,
    hasHooksFile: true,
    hasHookEntries: true,
    hasConfigEntry: true,
    hasTaskIdBinding: true,
    hasStatusMappings: true,
  };

  const verifiedOpenCodeStatus = {
    installed: true,
    hasPlugin: true,
    hasTaskIdBinding: true,
    hasStatusEndpoint: true,
    hasEventMappings: true,
    hasMainSessionGuard: true,
    hasDuplicateProgressGuard: true,
    hasRegisteredPlugin: true,
    hasDuplicateKanvibePlugins: false,
    hasExpectedHookServerUrl: true,
    hasReachableHookServer: true,
    targetPath: "/workspace/task",
    pluginPath: "/workspace/task/.opencode/plugins/kanvibe-plugin.ts",
    boundTaskId: "task-1",
    registeredPluginUrls: [
      "file:///workspace/task/.opencode/plugins/kanvibe-plugin.ts",
    ],
  };

  const incompleteOpenCodeStatus = {
    installed: false,
    hasPlugin: true,
    hasTaskIdBinding: true,
    hasStatusEndpoint: true,
    hasEventMappings: true,
    hasMainSessionGuard: true,
    hasDuplicateProgressGuard: false,
    hasRegisteredPlugin: false,
    hasDuplicateKanvibePlugins: false,
    hasExpectedHookServerUrl: false,
    hasReachableHookServer: false,
    targetPath: "/workspace/task",
    pluginPath: "/workspace/task/.opencode/plugins/kanvibe-plugin.ts",
    registeredPluginUrls: [],
  };

  it("should not render when isOpen is false", () => {
    // Given
    const props = {
      isOpen: false,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: null,
      geminiStatus: null,
      codexStatus: null,
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    const { container } = renderDialog(props);

    // Then
    expect(container.firstChild).toBeNull();
  });

  it("should render when isOpen is true", () => {
    // Given
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: null,
      geminiStatus: null,
      codexStatus: null,
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    renderDialog(props);

    // Then
    expect(screen.getByText("hooksStatus")).toBeTruthy();
    expect(screen.getByText("Claude")).toBeTruthy();
  });

  it("should still show install action for remote tasks", () => {
    // Given
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: null,
      geminiStatus: null,
      codexStatus: null,
      openCodeStatus: null,
      isRemote: true,
    };

    // When
    renderDialog(props);

    // Then
    expect(screen.getAllByText("installHooks").length).toBeGreaterThan(0);
  });

  it("should keep other hook install buttons visually stable while one tool installs", async () => {
    // Given
    let resolveInstall: ((value: { success: boolean; status: typeof verifiedClaudeStatus }) => void) | undefined;
    mockInstallTaskHooks.mockImplementation(() => new Promise((resolve) => {
      resolveInstall = resolve;
    }));
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: null,
      geminiStatus: null,
      codexStatus: null,
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    renderDialog(props);
    const installButtons = screen.getAllByText("installHooks");
    fireEvent.click(installButtons[0]);

    // Then
    expect(screen.getByText("installingHooks")).toBeTruthy();
    expect(installButtons[1].hasAttribute("disabled")).toBe(false);

    // Cleanup
    resolveInstall?.({ success: true, status: verifiedClaudeStatus });
    await waitFor(() => {
      expect(screen.getByText("hooksInstallSuccess")).toBeTruthy();
    });
  });

  it("should allow another tool install to start while one install is still pending", async () => {
    let resolveClaudeInstall: ((value: { success: boolean; status: typeof verifiedClaudeStatus }) => void) | undefined;
    mockInstallTaskHooks.mockImplementation(() => new Promise((resolve) => {
      resolveClaudeInstall = resolve;
    }));
    mockInstallTaskGeminiHooks.mockResolvedValue({ success: true, status: verifiedGeminiStatus });

    renderDialog({
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: null,
      geminiStatus: null,
      codexStatus: null,
      openCodeStatus: null,
      isRemote: false,
    });

    const installButtons = screen.getAllByText("installHooks");
    fireEvent.click(installButtons[0]);
    fireEvent.click(installButtons[1]);

    await waitFor(() => {
      expect(mockInstallTaskGeminiHooks).toHaveBeenCalledWith("task-1");
    });

    resolveClaudeInstall?.({ success: true, status: verifiedClaudeStatus });
    await waitFor(() => {
      expect(screen.getByText("geminiHooksInstallSuccess")).toBeTruthy();
    });
  });

  it("should keep the close button usable while installs are running", async () => {
    let resolveInstall: ((value: { success: boolean; status: typeof verifiedClaudeStatus }) => void) | undefined;
    mockInstallTaskHooks.mockImplementation(() => new Promise((resolve) => {
      resolveInstall = resolve;
    }));
    const onClose = vi.fn();

    renderDialog({
      isOpen: true,
      onClose,
      taskId: "task-1",
      claudeStatus: null,
      geminiStatus: null,
      codexStatus: null,
      openCodeStatus: null,
      isRemote: false,
    });

    fireEvent.click(screen.getAllByText("installHooks")[0]);
    fireEvent.click(screen.getByText("hooksStatusDialog.close"));

    expect(onClose).toHaveBeenCalledTimes(1);

    resolveInstall?.({ success: true, status: verifiedClaudeStatus });
    await waitFor(() => {
      expect(screen.getByText("hooksInstallSuccess")).toBeTruthy();
    });
  });

  it("should call onClose when close button is clicked", () => {
    // Given
    const onClose = vi.fn();
    const props = {
      isOpen: true,
      onClose,
      taskId: "task-1",
      claudeStatus: null,
      geminiStatus: null,
      codexStatus: null,
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    renderDialog(props);
    const closeButton = screen.getByText("hooksStatusDialog.close");
    fireEvent.click(closeButton);

    // Then
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("should call installTaskHooks when Claude install button is clicked", async () => {
    // Given
    mockInstallTaskHooks.mockResolvedValue({ success: true, status: verifiedClaudeStatus });
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: null,
      geminiStatus: null,
      codexStatus: null,
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    renderDialog(props);
    fireEvent.click(screen.getAllByText("installHooks")[0]);

    // Then
    await waitFor(() => {
      expect(mockInstallTaskHooks).toHaveBeenCalledWith("task-1");
    });
  });

  it("should show success message when Claude hooks installation succeeds", async () => {
    // Given
    mockInstallTaskHooks.mockResolvedValue({ success: true, status: verifiedClaudeStatus });
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: null,
      geminiStatus: null,
      codexStatus: null,
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    renderDialog(props);
    fireEvent.click(screen.getAllByText("installHooks")[0]);

    // Then
    await waitFor(() => {
      expect(screen.getByText("hooksInstallSuccess")).toBeTruthy();
    });
  });

  it("should show error message when Claude hooks installation fails", async () => {
    // Given
    mockInstallTaskHooks.mockResolvedValue({ success: false });
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: null,
      geminiStatus: null,
      codexStatus: null,
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    renderDialog(props);
    fireEvent.click(screen.getAllByText("installHooks")[0]);

    // Then
    await waitFor(() => {
      expect(screen.getByText("hooksInstallFailed")).toBeTruthy();
    });
  });

  it("should call installTaskGeminiHooks when Gemini install button is clicked", async () => {
    // Given
    mockInstallTaskGeminiHooks.mockResolvedValue({ success: true, status: verifiedGeminiStatus });
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: verifiedClaudeStatus,
      geminiStatus: null,
      codexStatus: null,
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    renderDialog(props);
    fireEvent.click(screen.getAllByText("installHooks")[0]);

    // Then
    await waitFor(() => {
      expect(mockInstallTaskGeminiHooks).toHaveBeenCalledWith("task-1");
    });
  });

  it("should show Gemini success message when Gemini hooks installation succeeds", async () => {
    // Given
    mockInstallTaskGeminiHooks.mockResolvedValue({ success: true, status: verifiedGeminiStatus });
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: verifiedClaudeStatus,
      geminiStatus: null,
      codexStatus: null,
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    renderDialog(props);
    fireEvent.click(screen.getAllByText("installHooks")[0]);

    // Then
    await waitFor(() => {
      expect(screen.getByText("geminiHooksInstallSuccess")).toBeTruthy();
    });
  });

  it("should call installTaskCodexHooks when Codex install button is clicked", async () => {
    // Given
    mockInstallTaskCodexHooks.mockResolvedValue({ success: true, status: verifiedCodexStatus });
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: verifiedClaudeStatus,
      geminiStatus: verifiedGeminiStatus,
      codexStatus: null,
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    renderDialog(props);
    fireEvent.click(screen.getAllByText("installHooks")[0]);

    // Then
    await waitFor(() => {
      expect(mockInstallTaskCodexHooks).toHaveBeenCalledWith("task-1");
    });
  });

  it("should show Codex success message when Codex hooks installation succeeds", async () => {
    // Given
    mockInstallTaskCodexHooks.mockResolvedValue({ success: true, status: verifiedCodexStatus });
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: verifiedClaudeStatus,
      geminiStatus: verifiedGeminiStatus,
      codexStatus: null,
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    renderDialog(props);
    fireEvent.click(screen.getAllByText("installHooks")[0]);

    // Then
    await waitFor(() => {
      expect(screen.getByText("codexHooksInstallSuccess")).toBeTruthy();
    });
  });

  it("should call installTaskOpenCodeHooks when OpenCode install button is clicked", async () => {
    // Given
    mockInstallTaskOpenCodeHooks.mockResolvedValue({ success: true, status: verifiedOpenCodeStatus });
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: verifiedClaudeStatus,
      geminiStatus: verifiedGeminiStatus,
      codexStatus: verifiedCodexStatus,
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    renderDialog(props);
    fireEvent.click(screen.getAllByText("installHooks")[0]);

    // Then
    await waitFor(() => {
      expect(mockInstallTaskOpenCodeHooks).toHaveBeenCalledWith("task-1");
    });
  });

  it("should show OpenCode success message when OpenCode hooks installation succeeds", async () => {
    // Given
    mockInstallTaskOpenCodeHooks.mockResolvedValue({ success: true, status: verifiedOpenCodeStatus });
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: verifiedClaudeStatus,
      geminiStatus: verifiedGeminiStatus,
      codexStatus: verifiedCodexStatus,
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    renderDialog(props);
    fireEvent.click(screen.getAllByText("installHooks")[0]);

    // Then
    await waitFor(() => {
      expect(screen.getByText("openCodeHooksInstallSuccess")).toBeTruthy();
    });
  });

  it("should show incomplete verification message when OpenCode install needs follow-up checks", async () => {
    // Given
    mockInstallTaskOpenCodeHooks.mockResolvedValue({ success: true, status: incompleteOpenCodeStatus });
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: verifiedClaudeStatus,
      geminiStatus: verifiedGeminiStatus,
      codexStatus: verifiedCodexStatus,
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    renderDialog(props);
    fireEvent.click(screen.getAllByText("installHooks")[0]);

    // Then
    await waitFor(() => {
      expect(screen.getByText("hooksInstallIncomplete")).toBeTruthy();
    });
  });

  it("should hide internal verification labels from users", () => {
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: verifiedClaudeStatus,
      geminiStatus: verifiedGeminiStatus,
      codexStatus: verifiedCodexStatus,
      openCodeStatus: null,
      isRemote: false,
    };

    renderDialog(props);

    expect(screen.queryByText("notify")).toBeNull();
    expect(screen.queryByText("config")).toBeNull();
    expect(screen.queryByText("event")).toBeNull();
    expect(screen.queryByText("plugin")).toBeNull();
    expect(screen.queryByText("dedupe")).toBeNull();
    expect(screen.queryByText("hooksBoundTaskId")).toBeNull();
  });

  it("should re-fetch OpenCode status when the dialog opens", async () => {
    const onStatusesChange = vi.fn();
    mockGetTaskOpenCodeHooksStatus.mockResolvedValue(verifiedOpenCodeStatus);

    renderDialog({
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: verifiedClaudeStatus,
      geminiStatus: verifiedGeminiStatus,
      codexStatus: verifiedCodexStatus,
      openCodeStatus: null,
      isRemote: false,
      onStatusesChange,
    });

    await waitFor(() => {
      expect(mockGetTaskOpenCodeHooksStatus).toHaveBeenCalledWith("task-1");
      expect(onStatusesChange).toHaveBeenCalledWith({ openCodeStatus: verifiedOpenCodeStatus });
    });
  });

  it("should keep OpenCode UI brief without showing the duplicate plugin warning", async () => {
    mockGetTaskOpenCodeHooksStatus.mockResolvedValue({
      ...verifiedOpenCodeStatus,
      hasDuplicateKanvibePlugins: true,
      registeredPluginUrls: [
        "file:///workspace/task/.opencode/plugins/kanvibe-plugin.ts",
        "file:///home/test/.config/opencode/plugins/kanvibe-plugin.ts",
      ],
    });

    renderDialog({
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: verifiedClaudeStatus,
      geminiStatus: verifiedGeminiStatus,
      codexStatus: verifiedCodexStatus,
      openCodeStatus: null,
      isRemote: false,
    });

    await waitFor(() => {
      expect(mockGetTaskOpenCodeHooksStatus).toHaveBeenCalledWith("task-1");
    });

    expect(screen.getAllByText("hooksInstalled").length).toBeGreaterThan(0);
    expect(screen.queryByText("hooksOpenCodeDiagnosticsTitle")).toBeNull();
    expect(screen.queryByText("hooksDiagnosticTargetPath")).toBeNull();
    expect(screen.queryByText("hooksDiagnosticPluginPath")).toBeNull();
    expect(screen.queryByText("hooksDiagnosticDuplicatePluginsHelp")).toBeNull();
    expect(screen.queryByText("hooksDiagnosticFailedChecks")).toBeNull();
  });

  it("should show installed status when hook is already installed", () => {
    // Given
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: verifiedClaudeStatus,
      geminiStatus: null,
      codexStatus: null,
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    renderDialog(props);

    // Then
    expect(screen.getByText("Claude")).toBeTruthy();
    const installedBadges = screen.getAllByText("hooksInstalled");
    expect(installedBadges.length).toBeGreaterThan(0);
  });

  it("should show reinstall action when hook is already installed", () => {
    // Given
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: verifiedClaudeStatus,
      geminiStatus: null,
      codexStatus: null,
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    renderDialog(props);

    // Then
    expect(screen.getByText("hooksStatusDialog.reinstall")).toBeTruthy();
  });

  it("should report updated hook status to the parent card after installation", async () => {
    const onStatusesChange = vi.fn();
    mockInstallTaskHooks.mockResolvedValue({ success: true, status: verifiedClaudeStatus });

    renderDialog({
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: null,
      geminiStatus: null,
      codexStatus: null,
      openCodeStatus: null,
      isRemote: false,
      onStatusesChange,
    });

    fireEvent.click(screen.getAllByText("installHooks")[0]);

    await waitFor(() => {
      expect(onStatusesChange).toHaveBeenCalledWith({ claudeStatus: verifiedClaudeStatus });
    });
  });
});
