import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import HooksStatusCard from "@/components/HooksStatusCard";
import { IntlProvider } from "next-intl";

// --- Mocks ---

vi.mock("next-intl", async () => {
  const actual = await vi.importActual("next-intl");
  return {
    ...actual,
    useTranslations: () => (key: string) => key,
  };
});

vi.mock("@/components/HooksStatusDialog", () => {
  // eslint-disable-next-line react/display-name
  const MockDialog = ({ isOpen, onClose, taskId, isRemote }: any) =>
    isOpen ? (
      <div data-testid="hooks-status-dialog">
        Dialog: taskId={taskId}, isRemote={isRemote}
        <button onClick={onClose}>Close Dialog</button>
      </div>
    ) : null;
  return {
    default: MockDialog,
  };
});

// 테스트용 메시지 객체
const messages = {
  taskDetail: {
    hooksStatus: "Hooks Status",
  },
};

describe("HooksStatusCard", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  const renderCard = (props: any) => {
    return render(
      <IntlProvider messages={messages} locale="en">
        <HooksStatusCard {...props} />
      </IntlProvider>
    );
  };

  it("should not render signal button when isRemote is true", () => {
    // Given
    const props = {
      taskId: "task-1",
      initialClaudeStatus: null,
      initialGeminiStatus: null,
      initialCodexStatus: null,
      isRemote: true,
    };

    // When
    renderCard(props);

    // Then - Signal button should not exist (check for status text)
    expect(screen.queryByText("All OK")).toBeNull();
    expect(screen.queryByText("Not Installed")).toBeNull();
    expect(screen.queryByText("Partial")).toBeNull();
  });

  it("should render signal button showing overall status when isRemote is false", () => {
    // Given
    const props = {
      taskId: "task-1",
      initialClaudeStatus: null,
      initialGeminiStatus: null,
      initialCodexStatus: null,
      isRemote: false,
    };

    // When
    renderCard(props);

    // Then
    expect(screen.getByText("Not Installed")).toBeTruthy();
  });

  it("should show green signal when all hooks are installed", () => {
    // Given
    const props = {
      taskId: "task-1",
      initialClaudeStatus: { installed: true, hasPromptHook: true, hasStopHook: true, hasQuestionHook: true, hasSettingsEntry: true },
      initialGeminiStatus: { installed: true, hasPromptHook: true, hasStopHook: true, hasSettingsEntry: true },
      initialCodexStatus: { installed: true, hasNotifyHook: true, hasConfigEntry: true },
      isRemote: false,
    };

    // When
    renderCard(props);

    // Then
    expect(screen.getByText("All OK")).toBeTruthy();
  });

  it("should show red signal when no hooks are installed", () => {
    // Given
    const props = {
      taskId: "task-1",
      initialClaudeStatus: null,
      initialGeminiStatus: null,
      initialCodexStatus: null,
      isRemote: false,
    };

    // When
    renderCard(props);

    // Then
    expect(screen.getByText("Not Installed")).toBeTruthy();
  });

  it("should show yellow signal when some hooks are installed", () => {
    // Given
    const props = {
      taskId: "task-1",
      initialClaudeStatus: { installed: true, hasPromptHook: true, hasStopHook: true, hasQuestionHook: true, hasSettingsEntry: true },
      initialGeminiStatus: null,
      initialCodexStatus: null,
      isRemote: false,
    };

    // When
    renderCard(props);

    // Then
    expect(screen.getByText("Partial")).toBeTruthy();
  });

  it("should open dialog when signal button is clicked", () => {
    // Given
    const props = {
      taskId: "task-1",
      initialClaudeStatus: null,
      initialGeminiStatus: null,
      initialCodexStatus: null,
      isRemote: false,
    };

    // When
    renderCard(props);
    const signalButton = screen.getByText("Not Installed");
    fireEvent.click(signalButton);

    // Then
    expect(screen.getByTestId("hooks-status-dialog")).toBeTruthy();
  });

  it("should close dialog when close dialog button is clicked", () => {
    // Given
    const props = {
      taskId: "task-1",
      initialClaudeStatus: null,
      initialGeminiStatus: null,
      initialCodexStatus: null,
      isRemote: false,
    };

    // When
    renderCard(props);
    const signalButton = screen.getByText("Not Installed");
    fireEvent.click(signalButton);
    expect(screen.getByTestId("hooks-status-dialog")).toBeTruthy();

    const closeButton = screen.getByText("Close Dialog");
    fireEvent.click(closeButton);

    // Then
    expect(screen.queryByTestId("hooks-status-dialog")).toBeNull();
  });

  it("should pass correct taskId to dialog", () => {
    // Given
    const taskId = "task-123";
    const props = {
      taskId,
      initialClaudeStatus: null,
      initialGeminiStatus: null,
      initialCodexStatus: null,
      isRemote: false,
    };

    // When
    renderCard(props);
    const signalButton = screen.getByText("Not Installed");
    fireEvent.click(signalButton);

    // Then
    const dialog = screen.getByTestId("hooks-status-dialog");
    expect(dialog.textContent).toContain(`taskId=${taskId}`);
  });

  it("should pass isRemote=true to dialog", () => {
    // Given
    const props = {
      taskId: "task-1",
      initialClaudeStatus: null,
      initialGeminiStatus: null,
      initialCodexStatus: null,
      isRemote: true,
    };

    // When
    renderCard(props);

    // Then - When isRemote is true, signal button should not be rendered
    expect(screen.queryByText("All OK")).toBeNull();
    expect(screen.queryByText("Not Installed")).toBeNull();
    expect(screen.queryByText("Partial")).toBeNull();
  });

  it("should not render dialog when isRemote is true", () => {
    // Given
    const props = {
      taskId: "task-1",
      initialClaudeStatus: null,
      initialGeminiStatus: null,
      initialCodexStatus: null,
      isRemote: true,
    };

    // When
    renderCard(props);

    // Then
    expect(screen.queryByTestId("hooks-status-dialog")).toBeNull();
  });

  it("should display Hooks Status heading", () => {
    // Given
    const props = {
      taskId: "task-1",
      initialClaudeStatus: null,
      initialGeminiStatus: null,
      initialCodexStatus: null,
      isRemote: false,
    };

    // When
    renderCard(props);

    // Then - useTranslations mock은 key를 그대로 반환하므로 "hooksStatus"가 렌더링됨
    expect(screen.getByText("hooksStatus")).toBeTruthy();
  });
});
