import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, cleanup, act } from "@testing-library/react";

// --- Mocks ---

const mockRefresh = vi.fn();

vi.mock("@/i18n/navigation", () => ({
  useRouter: () => ({
    refresh: mockRefresh,
    push: vi.fn(),
    replace: vi.fn(),
    back: vi.fn(),
    forward: vi.fn(),
    prefetch: vi.fn(),
  }),
}));

/** WebSocket 목 클래스. 인스턴스를 추적하여 테스트에서 메시지 수신을 시뮬레이션한다 */
class MockWebSocket {
  static instances: MockWebSocket[] = [];
  onmessage: ((event: MessageEvent) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  close = vi.fn();

  constructor() {
    MockWebSocket.instances.push(this);
  }
}

vi.stubGlobal("WebSocket", MockWebSocket);

describe("useAutoRefresh", () => {
  beforeEach(() => {
    /** 모듈 레벨 변수(boardHasMountedBefore)를 초기화하기 위해 모듈을 리셋한다 */
    vi.resetModules();
    mockRefresh.mockClear();
    MockWebSocket.instances = [];
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("should not call router.refresh() on first mount", async () => {
    // Given
    const { useAutoRefresh } = await import("@/hooks/useAutoRefresh");

    // When
    renderHook(() => useAutoRefresh());

    // Then
    expect(mockRefresh).not.toHaveBeenCalled();
  });

  it("should call router.refresh() on remount to invalidate router cache", async () => {
    // Given
    const { useAutoRefresh } = await import("@/hooks/useAutoRefresh");
    const { unmount } = renderHook(() => useAutoRefresh());
    expect(mockRefresh).not.toHaveBeenCalled();

    // When
    unmount();
    renderHook(() => useAutoRefresh());

    // Then
    expect(mockRefresh).toHaveBeenCalledTimes(1);
  });

  it("should call router.refresh() when receiving board-updated WebSocket message", async () => {
    // Given
    const { useAutoRefresh } = await import("@/hooks/useAutoRefresh");
    renderHook(() => useAutoRefresh());
    const ws = MockWebSocket.instances[0];

    // When
    act(() => {
      ws.onmessage?.({
        data: JSON.stringify({ type: "board-updated" }),
      } as MessageEvent);
    });

    // Then
    expect(mockRefresh).toHaveBeenCalledTimes(1);
  });

  it("should not call router.refresh() for non board-updated messages", async () => {
    // Given
    const { useAutoRefresh } = await import("@/hooks/useAutoRefresh");
    renderHook(() => useAutoRefresh());
    const ws = MockWebSocket.instances[0];

    // When
    act(() => {
      ws.onmessage?.({
        data: JSON.stringify({ type: "task-updated" }),
      } as MessageEvent);
    });

    // Then
    expect(mockRefresh).not.toHaveBeenCalled();
  });

  it("should attempt reconnection after WebSocket close", async () => {
    // Given
    vi.useFakeTimers();
    const { useAutoRefresh } = await import("@/hooks/useAutoRefresh");
    renderHook(() => useAutoRefresh());
    expect(MockWebSocket.instances).toHaveLength(1);

    // When
    act(() => {
      MockWebSocket.instances[0].onclose?.();
    });

    // Then
    act(() => {
      vi.advanceTimersByTime(3000);
    });
    expect(MockWebSocket.instances).toHaveLength(2);

    vi.useRealTimers();
  });

  it("should close WebSocket connection on unmount", async () => {
    // Given
    const { useAutoRefresh } = await import("@/hooks/useAutoRefresh");
    const { unmount } = renderHook(() => useAutoRefresh());
    const ws = MockWebSocket.instances[0];

    // When
    unmount();

    // Then
    expect(ws.close).toHaveBeenCalled();
  });
});
