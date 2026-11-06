import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import ConnectTerminalForm from "../ConnectTerminalForm";
import { SessionType } from "@/entities/KanbanTask";

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
      expect(mockEnsureSessionDependencyWithPrompt).toHaveBeenCalledWith("herdr", "remote-box", expect.any(Function));
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
  it("세션을 고르지 않은 연결 화면은 Herdr를 선택한다", () => {
    render(<ConnectTerminalForm taskId="task-1" />);
    expect((screen.getByRole("combobox") as HTMLSelectElement).value).toBe("herdr");
  });

  it.each([SessionType.TMUX, SessionType.ZELLIJ, SessionType.TERMINAL])("저장된 기본값 %s를 연결 화면에서도 유지한다", (sessionType) => {
    render(<ConnectTerminalForm taskId="task-1" defaultSessionType={sessionType} />);
    expect((screen.getByRole("combobox") as HTMLSelectElement).value).toBe(sessionType);
  });

  it("늦게 읽은 저장 기본값을 사용자가 선택하기 전 화면과 연결 요청에 반영한다", async () => {
    mockEnsureSessionDependencyWithPrompt.mockResolvedValue(true);
    mockConnectTerminalSession.mockResolvedValue(null);
    const { rerender } = render(<ConnectTerminalForm taskId="task-1" defaultSessionType={SessionType.HERDR} />);
    const select = screen.getByRole("combobox") as HTMLSelectElement;
    expect(select.value).toBe("herdr");

    rerender(<ConnectTerminalForm taskId="task-1" defaultSessionType={SessionType.TMUX} />);
    expect(select.value).toBe("tmux");
    fireEvent.click(screen.getByRole("button", { name: "connectTerminal" }));
    await waitFor(() => expect(mockConnectTerminalSession).toHaveBeenCalledWith("task-1", "tmux"));
  });

  it.each([SessionType.ZELLIJ, SessionType.HERDR])("직접 고른 %s는 저장 기본값이 늦게 도착해도 포커스와 함께 유지한다", async (sessionType) => {
    mockEnsureSessionDependencyWithPrompt.mockResolvedValue(true);
    mockConnectTerminalSession.mockResolvedValue(null);
    const { rerender } = render(<ConnectTerminalForm taskId="task-1" defaultSessionType={SessionType.HERDR} />);
    const select = screen.getByRole("combobox") as HTMLSelectElement;
    select.focus();
    fireEvent.change(select, { target: { value: "zellij" } });
    fireEvent.change(select, { target: { value: sessionType } });

    rerender(<ConnectTerminalForm taskId="task-1" defaultSessionType={SessionType.TMUX} />);
    expect(screen.getByRole("combobox")).toBe(select);
    expect(document.activeElement).toBe(select);
    expect(select.value).toBe(sessionType);
    fireEvent.click(screen.getByRole("button", { name: "connectTerminal" }));
    await waitFor(() => expect(mockConnectTerminalSession).toHaveBeenCalledWith("task-1", sessionType));
  });

});
