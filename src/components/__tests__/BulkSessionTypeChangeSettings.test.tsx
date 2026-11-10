import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import BulkSessionTypeChangeSettings from "../BulkSessionTypeChangeSettings";
import { SessionType } from "@/entities/KanbanTask";

const mocks = vi.hoisted(() => ({
  getSessionTypeChangeTargets: vi.fn(),
  changeAllTaskSessionTypes: vi.fn(),
  ensureSessionDependencyWithPrompt: vi.fn(),
}));

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, string | number>) => (
    values ? `${key}:${JSON.stringify(values)}` : key
  ),
}));

vi.mock("@/desktop/renderer/actions/kanban", () => ({
  getSessionTypeChangeTargets: (...args: unknown[]) => mocks.getSessionTypeChangeTargets(...args),
  changeAllTaskSessionTypes: (...args: unknown[]) => mocks.changeAllTaskSessionTypes(...args),
}));

vi.mock("@/desktop/renderer/utils/sessionDependencyPrompt", () => ({
  ensureSessionDependencyWithPrompt: (...args: unknown[]) => mocks.ensureSessionDependencyWithPrompt(...args),
}));

function applyBulkChange(sessionType: SessionType) {
  fireEvent.change(screen.getByRole("combobox"), { target: { value: sessionType } });
  fireEvent.click(screen.getByRole("button", { name: "bulkSessionTypeApply" }));
}

describe("BulkSessionTypeChangeSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.ensureSessionDependencyWithPrompt.mockResolvedValue(true);
    mocks.changeAllTaskSessionTypes.mockResolvedValue({ changedCount: 3, failedCount: 0 });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("바꿀 task가 없으면 묻지도 바꾸지도 않고 안내만 한다", async () => {
    // Given
    mocks.getSessionTypeChangeTargets.mockResolvedValue({ taskCount: 0, liveSessionCount: 0, sshHosts: [] });
    const confirmSpy = vi.spyOn(window, "confirm");
    render(<BulkSessionTypeChangeSettings defaultSessionType={SessionType.TMUX} />);

    // When
    applyBulkChange(SessionType.ZELLIJ);

    // Then
    await waitFor(() => {
      expect(screen.getByText("bulkSessionTypeNoTargets")).toBeTruthy();
    });
    expect(confirmSpy).not.toHaveBeenCalled();
    expect(mocks.changeAllTaskSessionTypes).not.toHaveBeenCalled();
  });

  it("몇 개를 바꿀지 묻는 확인을 취소하면 바꾸지 않는다", async () => {
    // Given
    mocks.getSessionTypeChangeTargets.mockResolvedValue({ taskCount: 3, liveSessionCount: 2, sshHosts: [null] });
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<BulkSessionTypeChangeSettings defaultSessionType={SessionType.TMUX} />);

    // When
    applyBulkChange(SessionType.ZELLIJ);

    // Then
    await waitFor(() => {
      expect(confirmSpy).toHaveBeenCalledWith('bulkSessionTypeChangeConfirm:{"count":3,"sessionType":"zellij"}');
    });
    expect(mocks.ensureSessionDependencyWithPrompt).not.toHaveBeenCalled();
    expect(mocks.changeAllTaskSessionTypes).not.toHaveBeenCalled();
  });

  it("변경을 확인한 뒤 켜진 세션을 닫을지 묻고 그 답을 그대로 넘기며 결과를 보여 준다", async () => {
    // Given
    mocks.getSessionTypeChangeTargets.mockResolvedValue({ taskCount: 3, liveSessionCount: 2, sshHosts: [null, "build-host"] });
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValueOnce(true).mockReturnValueOnce(false);
    render(<BulkSessionTypeChangeSettings defaultSessionType={SessionType.TMUX} />);

    // When
    applyBulkChange(SessionType.ZELLIJ);

    // Then
    await waitFor(() => {
      expect(mocks.changeAllTaskSessionTypes).toHaveBeenCalledWith("zellij", false);
    });
    expect(confirmSpy).toHaveBeenNthCalledWith(2, 'bulkCloseExistingSessionsConfirm:{"count":2}');
    expect(mocks.ensureSessionDependencyWithPrompt).toHaveBeenCalledWith("zellij", null, expect.any(Function));
    expect(mocks.ensureSessionDependencyWithPrompt).toHaveBeenCalledWith("zellij", "build-host", expect.any(Function));
    expect(await screen.findByText('bulkSessionTypeChangeResult:{"changedCount":3,"failedCount":0}')).toBeTruthy();
  });

  it("켜진 세션이 없으면 닫기 확인 없이 바꾼다", async () => {
    // Given
    mocks.getSessionTypeChangeTargets.mockResolvedValue({ taskCount: 1, liveSessionCount: 0, sshHosts: [null] });
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<BulkSessionTypeChangeSettings defaultSessionType={SessionType.TMUX} />);

    // When
    applyBulkChange(SessionType.HERDR);

    // Then
    await waitFor(() => {
      expect(mocks.changeAllTaskSessionTypes).toHaveBeenCalledWith("herdr", false);
    });
    expect(confirmSpy).toHaveBeenCalledTimes(1);
  });

  it("어느 호스트든 새 세션 도구 준비를 거절하면 바꾸지 않는다", async () => {
    // Given
    mocks.getSessionTypeChangeTargets.mockResolvedValue({ taskCount: 2, liveSessionCount: 0, sshHosts: [null, "build-host"] });
    vi.spyOn(window, "confirm").mockReturnValue(true);
    mocks.ensureSessionDependencyWithPrompt.mockImplementation(async (_type: string, host: string | null) => host === null);
    render(<BulkSessionTypeChangeSettings defaultSessionType={SessionType.TMUX} />);

    // When
    applyBulkChange(SessionType.ZELLIJ);

    // Then
    await waitFor(() => {
      expect(mocks.ensureSessionDependencyWithPrompt).toHaveBeenCalledTimes(2);
    });
    expect(mocks.changeAllTaskSessionTypes).not.toHaveBeenCalled();
  });
});
