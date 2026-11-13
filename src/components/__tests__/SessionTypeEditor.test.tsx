import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import SessionTypeEditor from "../SessionTypeEditor";
import { SessionType } from "@/entities/KanbanTask";

const mocks = vi.hoisted(() => ({
  changeTaskSessionType: vi.fn(),
  isTaskSessionAlive: vi.fn(),
  ensureSessionDependencyWithPrompt: vi.fn(),
  refresh: vi.fn(),
}));

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, string>) => (
    values ? `${key}:${JSON.stringify(values)}` : key
  ),
}));

vi.mock("@/desktop/renderer/navigation", () => ({
  useRouter: () => ({ refresh: mocks.refresh }),
}));

vi.mock("@/desktop/renderer/actions/kanban", () => ({
  changeTaskSessionType: (...args: unknown[]) => mocks.changeTaskSessionType(...args),
  isTaskSessionAlive: (...args: unknown[]) => mocks.isTaskSessionAlive(...args),
}));

vi.mock("@/desktop/renderer/utils/sessionDependencyPrompt", () => ({
  ensureSessionDependencyWithPrompt: (...args: unknown[]) => mocks.ensureSessionDependencyWithPrompt(...args),
}));

describe("SessionTypeEditor", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.ensureSessionDependencyWithPrompt.mockResolvedValue(true);
    mocks.changeTaskSessionType.mockResolvedValue({ id: "task-1", sessionType: "zellij" });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("의존성 확인과 변경을 기다리는 동안 상태를 표시하고 중복 제출을 막는다", async () => {
    let resolveDependency!: (ready: boolean) => void;
    let resolveChange!: (task: { id: string }) => void;
    mocks.ensureSessionDependencyWithPrompt.mockReturnValue(new Promise<boolean>((resolve) => { resolveDependency = resolve; }));
    mocks.changeTaskSessionType.mockReturnValue(new Promise<{ id: string }>((resolve) => { resolveChange = resolve; }));
    mocks.isTaskSessionAlive.mockResolvedValue(false);
    render(<SessionTypeEditor taskId="task-1" currentSessionType={SessionType.HERDR} sshHost="build-host" />);

    fireEvent.change(screen.getByRole("combobox"), { target: { value: "tmux" } });
    expect(screen.getByRole("status").textContent).toBe("sessionTypeChanging");
    expect((screen.getByRole("combobox") as HTMLSelectElement).disabled).toBe(true);
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "zellij" } });
    expect(mocks.ensureSessionDependencyWithPrompt).toHaveBeenCalledTimes(1);
    await act(async () => { resolveDependency(true); });
    expect(screen.getByRole("status")).toBeTruthy();
    await act(async () => { resolveChange({ id: "task-1" }); });
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
    expect(mocks.changeTaskSessionType).toHaveBeenCalledTimes(1);
  });

  it("변경 예외를 그대로 표시하고 파괴적인 변경을 자동 반복하지 않는다", async () => {
    mocks.isTaskSessionAlive.mockResolvedValue(false);
    mocks.changeTaskSessionType.mockRejectedValue(new Error("SSH transport failure"));
    render(<SessionTypeEditor taskId="task-1" currentSessionType={SessionType.HERDR} sshHost="build-host" />);
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "tmux" } });
    await waitFor(() => expect(screen.getByRole("alert").textContent).toBe("SSH transport failure"));
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
    expect(mocks.changeTaskSessionType).toHaveBeenCalledTimes(1);
  });

  it("기존 세션이 켜져 있으면 닫을지 묻고 확인하면 기존 세션을 닫으며 바꾼다", async () => {
    // Given
    mocks.isTaskSessionAlive.mockResolvedValue(true);
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<SessionTypeEditor taskId="task-1" currentSessionType={SessionType.TMUX} sshHost="build-host" />);

    // When
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "zellij" } });

    // Then
    await waitFor(() => {
      expect(mocks.changeTaskSessionType).toHaveBeenCalledWith("task-1", "zellij", true);
    });
    expect(mocks.ensureSessionDependencyWithPrompt).toHaveBeenCalledWith("zellij", "build-host", expect.any(Function));
    expect(confirmSpy).toHaveBeenCalledWith('closeExistingSessionConfirm:{"sessionType":"tmux"}');
    expect(mocks.refresh).toHaveBeenCalled();
  });

  it("닫지 않기를 고르면 기존 세션을 남긴 채 바꾼다", async () => {
    // Given
    mocks.isTaskSessionAlive.mockResolvedValue(true);
    vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<SessionTypeEditor taskId="task-1" currentSessionType={SessionType.TMUX} sshHost={null} />);

    // When
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "zellij" } });

    // Then
    await waitFor(() => {
      expect(mocks.changeTaskSessionType).toHaveBeenCalledWith("task-1", "zellij", false);
    });
  });

  it("기존 세션이 꺼져 있으면 묻지 않고 바꾼다", async () => {
    // Given
    mocks.isTaskSessionAlive.mockResolvedValue(false);
    const confirmSpy = vi.spyOn(window, "confirm");
    render(<SessionTypeEditor taskId="task-1" currentSessionType={SessionType.TMUX} sshHost={null} />);

    // When
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "herdr" } });

    // Then
    await waitFor(() => {
      expect(mocks.changeTaskSessionType).toHaveBeenCalledWith("task-1", "herdr", false);
    });
    expect(confirmSpy).not.toHaveBeenCalled();
  });

  it("새 세션 도구 설치를 거절하면 바꾸지 않는다", async () => {
    // Given
    mocks.ensureSessionDependencyWithPrompt.mockResolvedValue(false);
    render(<SessionTypeEditor taskId="task-1" currentSessionType={SessionType.TMUX} sshHost={null} />);

    // When
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "zellij" } });

    // Then
    await waitFor(() => {
      expect(mocks.ensureSessionDependencyWithPrompt).toHaveBeenCalled();
    });
    expect(mocks.isTaskSessionAlive).not.toHaveBeenCalled();
    expect(mocks.changeTaskSessionType).not.toHaveBeenCalled();
  });

  it("변경에 실패하면 오류를 보여 준다", async () => {
    // Given
    mocks.isTaskSessionAlive.mockResolvedValue(false);
    mocks.changeTaskSessionType.mockResolvedValue(null);
    render(<SessionTypeEditor taskId="task-1" currentSessionType={SessionType.TMUX} sshHost={null} />);

    // When
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "zellij" } });

    // Then
    await waitFor(() => {
      expect(screen.getByText("sessionTypeChangeFailed")).toBeTruthy();
    });
    expect(mocks.refresh).not.toHaveBeenCalled();
  });
});
