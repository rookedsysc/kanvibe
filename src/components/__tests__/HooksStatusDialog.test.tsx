import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import HooksStatusDialog from "@/components/HooksStatusDialog";

// --- Mocks ---

const {
  mockInstallTaskHooks,
  mockInstallTaskGeminiHooks,
  mockInstallTaskCodexHooks,
  mockInstallTaskOpenCodeHooks,
} = vi.hoisted(() => ({
  mockInstallTaskHooks: vi.fn(),
  mockInstallTaskGeminiHooks: vi.fn(),
  mockInstallTaskCodexHooks: vi.fn(),
  mockInstallTaskOpenCodeHooks: vi.fn(),
}));

vi.mock("@/app/actions/project", () => ({
  installTaskHooks: mockInstallTaskHooks,
  installTaskGeminiHooks: mockInstallTaskGeminiHooks,
  installTaskCodexHooks: mockInstallTaskCodexHooks,
  installTaskOpenCodeHooks: mockInstallTaskOpenCodeHooks,
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
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  const renderDialog = (props: any) => {
    return render(<HooksStatusDialog {...props} />);
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

  it("should show remote not supported message when isRemote is true", () => {
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
    const remoteMessages = screen.getAllByText("hooksRemoteNotSupported");
    expect(remoteMessages.length).toBeGreaterThan(0);
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
    mockInstallTaskHooks.mockResolvedValue({ success: true });
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
    await waitFor(() => {
      expect(mockInstallTaskHooks).toHaveBeenCalledWith("task-1");
    });
  });

  it("should show success message when Claude hooks installation succeeds", async () => {
    // Given
    mockInstallTaskHooks.mockResolvedValue({ success: true });
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
    const installButtons = screen.getAllByText("installHooks");
    fireEvent.click(installButtons[0]);

    // Then
    await waitFor(() => {
      expect(screen.getByText("hooksInstallFailed")).toBeTruthy();
    });
  });

  it("should call installTaskGeminiHooks when Gemini install button is clicked", async () => {
    // Given
    mockInstallTaskGeminiHooks.mockResolvedValue({ success: true });
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: { installed: true, hasPromptHook: true, hasStopHook: true, hasQuestionHook: true, hasSettingsEntry: true },
      geminiStatus: null,
      codexStatus: null,
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    renderDialog(props);
    const installButtons = screen.getAllByText("installHooks");
    fireEvent.click(installButtons[1]);

    // Then
    await waitFor(() => {
      expect(mockInstallTaskGeminiHooks).toHaveBeenCalledWith("task-1");
    });
  });

  it("should show Gemini success message when Gemini hooks installation succeeds", async () => {
    // Given
    mockInstallTaskGeminiHooks.mockResolvedValue({ success: true });
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: { installed: true, hasPromptHook: true, hasStopHook: true, hasQuestionHook: true, hasSettingsEntry: true },
      geminiStatus: null,
      codexStatus: null,
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    renderDialog(props);
    const installButtons = screen.getAllByText("installHooks");
    fireEvent.click(installButtons[1]);

    // Then
    await waitFor(() => {
      expect(screen.getByText("geminiHooksInstallSuccess")).toBeTruthy();
    });
  });

  it("should call installTaskCodexHooks when Codex install button is clicked", async () => {
    // Given
    mockInstallTaskCodexHooks.mockResolvedValue({ success: true });
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: { installed: true, hasPromptHook: true, hasStopHook: true, hasQuestionHook: true, hasSettingsEntry: true },
      geminiStatus: { installed: true, hasPromptHook: true, hasStopHook: true, hasSettingsEntry: true },
      codexStatus: null,
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    renderDialog(props);
    const installButtons = screen.getAllByText("installHooks");
    fireEvent.click(installButtons[2]);

    // Then
    await waitFor(() => {
      expect(mockInstallTaskCodexHooks).toHaveBeenCalledWith("task-1");
    });
  });

  it("should show Codex success message when Codex hooks installation succeeds", async () => {
    // Given
    mockInstallTaskCodexHooks.mockResolvedValue({ success: true });
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: { installed: true, hasPromptHook: true, hasStopHook: true, hasQuestionHook: true, hasSettingsEntry: true },
      geminiStatus: { installed: true, hasPromptHook: true, hasStopHook: true, hasSettingsEntry: true },
      codexStatus: null,
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    renderDialog(props);
    const installButtons = screen.getAllByText("installHooks");
    fireEvent.click(installButtons[2]);

    // Then
    await waitFor(() => {
      expect(screen.getByText("codexHooksInstallSuccess")).toBeTruthy();
    });
  });

  it("should call installTaskOpenCodeHooks when OpenCode install button is clicked", async () => {
    // Given
    mockInstallTaskOpenCodeHooks.mockResolvedValue({ success: true });
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: { installed: true, hasPromptHook: true, hasStopHook: true, hasQuestionHook: true, hasSettingsEntry: true },
      geminiStatus: { installed: true, hasPromptHook: true, hasStopHook: true, hasSettingsEntry: true },
      codexStatus: { installed: true, hasNotifyHook: true, hasConfigEntry: true },
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    renderDialog(props);
    const installButtons = screen.getAllByText("installHooks");
    fireEvent.click(installButtons[3]);

    // Then
    await waitFor(() => {
      expect(mockInstallTaskOpenCodeHooks).toHaveBeenCalledWith("task-1");
    });
  });

  it("should show OpenCode success message when OpenCode hooks installation succeeds", async () => {
    // Given
    mockInstallTaskOpenCodeHooks.mockResolvedValue({ success: true });
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: { installed: true, hasPromptHook: true, hasStopHook: true, hasQuestionHook: true, hasSettingsEntry: true },
      geminiStatus: { installed: true, hasPromptHook: true, hasStopHook: true, hasSettingsEntry: true },
      codexStatus: { installed: true, hasNotifyHook: true, hasConfigEntry: true },
      openCodeStatus: null,
      isRemote: false,
    };

    // When
    renderDialog(props);
    const installButtons = screen.getAllByText("installHooks");
    fireEvent.click(installButtons[3]);

    // Then
    await waitFor(() => {
      expect(screen.getByText("openCodeHooksInstallSuccess")).toBeTruthy();
    });
  });

  it("should show installed status when hook is already installed", () => {
    // Given
    const props = {
      isOpen: true,
      onClose: vi.fn(),
      taskId: "task-1",
      claudeStatus: { installed: true, hasPromptHook: true, hasStopHook: true, hasQuestionHook: true, hasSettingsEntry: true },
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

  it("should disable close button when installation is pending", async () => {
    // Given
    mockInstallTaskHooks.mockImplementation(
      () =>
        new Promise((resolve) =>
          setTimeout(() => resolve({ success: true }), 1000)
        )
    );
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
    await waitFor(() => {
      const closeButton = screen.getByText("hooksStatusDialog.close") as HTMLButtonElement;
      expect(closeButton.disabled).toBe(true);
    });
  });
});
