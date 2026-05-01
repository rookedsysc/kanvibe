import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import ConnectTerminalForm from "../ConnectTerminalForm";

const { mockConnectTerminalSession, mockEnsureSessionDependencyWithPrompt } = vi.hoisted(() => ({
  mockConnectTerminalSession: vi.fn(),
  mockEnsureSessionDependencyWithPrompt: vi.fn(),
}));

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string) => key,
}));

vi.mock("@/desktop/renderer/actions/kanban", () => ({
  connectTerminalSession: (...args: unknown[]) => mockConnectTerminalSession(...args),
}));

vi.mock("@/desktop/renderer/utils/sessionDependencyPrompt", () => ({
  ensureSessionDependencyWithPrompt: (...args: unknown[]) => mockEnsureSessionDependencyWithPrompt(...args),
}));

describe("ConnectTerminalForm", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("원격 의존성 프롬프트를 취소하면 터미널 연결을 진행하지 않는다", async () => {
    // Given
    mockEnsureSessionDependencyWithPrompt.mockResolvedValue(false);

    render(<ConnectTerminalForm taskId="task-1" sshHost="remote-box" />);

    // When
    fireEvent.click(screen.getByRole("button", { name: "connectTerminal" }));

    // Then
    await waitFor(() => {
      expect(mockEnsureSessionDependencyWithPrompt).toHaveBeenCalledWith("tmux", "remote-box", expect.any(Function));
    });
    expect(mockConnectTerminalSession).not.toHaveBeenCalled();
  });

  it("의존성 확인이 실패하면 오류를 보여주고 연결을 멈춘다", async () => {
    // Given
    mockEnsureSessionDependencyWithPrompt.mockRejectedValue(new Error("zellij 설치 실패"));

    render(<ConnectTerminalForm taskId="task-1" sshHost="remote-box" />);

    // When
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "zellij" } });
    fireEvent.click(screen.getByRole("button", { name: "connectTerminal" }));

    // Then
    await waitFor(() => {
      expect(screen.getByText("zellij 설치 실패")).toBeTruthy();
    });
    expect(mockConnectTerminalSession).not.toHaveBeenCalled();
  });

  it("의존성 준비가 끝나면 선택한 세션 타입으로 연결을 요청한다", async () => {
    // Given
    const handleConnected = vi.fn();
    mockEnsureSessionDependencyWithPrompt.mockResolvedValue(true);
    mockConnectTerminalSession.mockResolvedValue({
      id: "task-1",
      sessionType: "zellij",
      sessionName: "task-1-zellij",
    });

    render(<ConnectTerminalForm taskId="task-1" sshHost="remote-box" onConnected={handleConnected} />);

    // When
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "zellij" } });
    fireEvent.click(screen.getByRole("button", { name: "connectTerminal" }));

    // Then
    await waitFor(() => {
      expect(mockConnectTerminalSession).toHaveBeenCalledWith("task-1", "zellij");
    });
    expect(handleConnected).toHaveBeenCalledWith({
      id: "task-1",
      sessionType: "zellij",
      sessionName: "task-1-zellij",
    });
  });
});
