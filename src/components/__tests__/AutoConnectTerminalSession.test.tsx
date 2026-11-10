import { StrictMode } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import AutoConnectTerminalSession from "../AutoConnectTerminalSession";
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

const CONNECTED_TASK = { id: "task-1", sessionType: "tmux", sessionName: "task-1-tmux" };

describe("AutoConnectTerminalSession", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("화면에 나타나면 폼 없이 기본 세션 타입으로 바로 연결한다", async () => {
    // Given
    const handleConnected = vi.fn();
    mockEnsureSessionDependencyWithPrompt.mockResolvedValue(true);
    mockConnectTerminalSession.mockResolvedValue(CONNECTED_TASK);

    // When
    render(
      <AutoConnectTerminalSession
        taskId="task-1"
        sshHost="remote-box"
        defaultSessionType={SessionType.TMUX}
        onConnected={handleConnected}
      />,
    );

    // Then
    expect(screen.getByText("connecting")).toBeTruthy();
    expect(screen.queryByRole("combobox")).toBeNull();
    await waitFor(() => expect(handleConnected).toHaveBeenCalledWith(CONNECTED_TASK));
    expect(mockEnsureSessionDependencyWithPrompt).toHaveBeenCalledWith("tmux", "remote-box", expect.any(Function));
    expect(mockConnectTerminalSession).toHaveBeenCalledWith("task-1", "tmux");
  });

  it("StrictMode처럼 effect가 두 번 돌아도 같은 task 연결은 한 번만 요청한다", async () => {
    // Given
    const handleConnected = vi.fn();
    mockEnsureSessionDependencyWithPrompt.mockResolvedValue(true);
    mockConnectTerminalSession.mockResolvedValue(CONNECTED_TASK);

    // When
    render(
      <StrictMode>
        <AutoConnectTerminalSession taskId="task-1" defaultSessionType={SessionType.TMUX} onConnected={handleConnected} />
      </StrictMode>,
    );

    // Then
    await waitFor(() => expect(handleConnected).toHaveBeenCalled());
    expect(mockEnsureSessionDependencyWithPrompt).toHaveBeenCalledTimes(1);
    expect(mockConnectTerminalSession).toHaveBeenCalledTimes(1);
  });

  it("연결에 실패하면 오류를 보여 주고, 다시 시도 버튼으로 재연결한다", async () => {
    // Given
    const handleConnected = vi.fn();
    mockEnsureSessionDependencyWithPrompt.mockResolvedValue(true);
    mockConnectTerminalSession.mockResolvedValueOnce(null).mockResolvedValueOnce(CONNECTED_TASK);

    render(<AutoConnectTerminalSession taskId="task-1" defaultSessionType={SessionType.TMUX} onConnected={handleConnected} />);
    expect(await screen.findByText("connectFailed")).toBeTruthy();
    expect(handleConnected).not.toHaveBeenCalled();

    // When
    fireEvent.click(screen.getByRole("button", { name: "connectTerminal" }));

    // Then
    await waitFor(() => expect(handleConnected).toHaveBeenCalledWith(CONNECTED_TASK));
    expect(mockConnectTerminalSession).toHaveBeenCalledTimes(2);
  });

  it("의존성 확인에서 예외가 나면 그 메시지를 보여 주고 연결하지 않는다", async () => {
    // Given
    mockEnsureSessionDependencyWithPrompt.mockRejectedValue(new Error("zellij 설치 실패"));

    // When
    render(<AutoConnectTerminalSession taskId="task-1" defaultSessionType={SessionType.ZELLIJ} />);

    // Then
    expect(await screen.findByText("zellij 설치 실패")).toBeTruthy();
    expect(mockConnectTerminalSession).not.toHaveBeenCalled();
  });

  it("의존성 설치 안내를 거절하면 연결하지 않고 연결 버튼만 남긴다", async () => {
    // Given
    mockEnsureSessionDependencyWithPrompt.mockResolvedValue(false);

    // When
    render(<AutoConnectTerminalSession taskId="task-1" defaultSessionType={SessionType.HERDR} />);

    // Then
    expect(await screen.findByText("noTerminal")).toBeTruthy();
    expect(screen.getByRole("button", { name: "connectTerminal" })).toBeTruthy();
    expect(mockConnectTerminalSession).not.toHaveBeenCalled();
  });
});
