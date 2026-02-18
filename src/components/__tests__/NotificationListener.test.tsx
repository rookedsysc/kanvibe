import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, act } from "@testing-library/react";

// --- Mocks ---

const mockNotifyTaskStatusChanged = vi.fn();

vi.mock("@/hooks/useTaskNotification", () => ({
  useTaskNotification: () => ({
    notifyTaskStatusChanged: mockNotifyTaskStatusChanged,
  }),
}));

vi.mock("next-intl", () => ({
  useLocale: () => "ko",
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

// Mock Service Worker registration
Object.defineProperty(global.navigator, "serviceWorker", {
  value: {
    register: vi.fn().mockResolvedValue({}),
  },
  configurable: true,
});

/** 테스트용 기본 props (전체 상태 알림 활성화) */
const defaultProps = {
  isNotificationEnabled: true,
  enabledStatuses: ["todo", "progress", "pending", "review", "done"],
};

describe("NotificationListener", () => {
  beforeEach(() => {
    vi.resetModules();
    mockNotifyTaskStatusChanged.mockClear();
    MockWebSocket.instances = [];
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("should call notifyTaskStatusChanged when receiving task-status-changed message", async () => {
    // Given
    const { default: NotificationListener } = await import("@/components/NotificationListener");
    render(<NotificationListener {...defaultProps} />);
    const ws = MockWebSocket.instances[0];

    const wsMessage = {
      type: "task-status-changed",
      projectName: "kanvibe",
      branchName: "feat/test",
      taskTitle: "테스트",
      description: null,
      newStatus: "review",
      taskId: "task-123",
    };

    // When
    act(() => {
      ws.onmessage?.({
        data: JSON.stringify(wsMessage),
      } as MessageEvent);
    });

    // Then — type 필드는 destructuring에서 제외되고, locale은 NotificationListener에서 추가됨
    const { type: _, ...wsPayload } = wsMessage;
    expect(mockNotifyTaskStatusChanged).toHaveBeenCalledWith({
      ...wsPayload,
      locale: "ko",
    });
  });

  it("should not call notifyTaskStatusChanged for board-updated messages", async () => {
    // Given
    const { default: NotificationListener } = await import("@/components/NotificationListener");
    render(<NotificationListener {...defaultProps} />);
    const ws = MockWebSocket.instances[0];

    // When
    act(() => {
      ws.onmessage?.({
        data: JSON.stringify({ type: "board-updated" }),
      } as MessageEvent);
    });

    // Then
    expect(mockNotifyTaskStatusChanged).not.toHaveBeenCalled();
  });

  it("should attempt reconnection after WebSocket close", async () => {
    // Given
    vi.useFakeTimers();
    const { default: NotificationListener } = await import("@/components/NotificationListener");
    render(<NotificationListener {...defaultProps} />);
    expect(MockWebSocket.instances).toHaveLength(1);

    // When
    act(() => {
      MockWebSocket.instances[0].onclose?.();
    });
    act(() => {
      vi.advanceTimersByTime(3000);
    });

    // Then
    expect(MockWebSocket.instances).toHaveLength(2);

    vi.useRealTimers();
  });

  it("should close WebSocket connection on unmount", async () => {
    // Given
    const { default: NotificationListener } = await import("@/components/NotificationListener");
    const { unmount } = render(<NotificationListener {...defaultProps} />);
    const ws = MockWebSocket.instances[0];

    // When
    unmount();

    // Then
    expect(ws.close).toHaveBeenCalled();
  });

  it("should not notify when isNotificationEnabled is false", async () => {
    // Given
    const { default: NotificationListener } = await import("@/components/NotificationListener");
    render(
      <NotificationListener isNotificationEnabled={false} enabledStatuses={["progress", "review"]} />
    );
    const ws = MockWebSocket.instances[0];

    // When
    act(() => {
      ws.onmessage?.({
        data: JSON.stringify({
          type: "task-status-changed",
          projectName: "kanvibe",
          branchName: "feat/test",
          taskTitle: "테스트",
          description: null,
          newStatus: "review",
        }),
      } as MessageEvent);
    });

    // Then
    expect(mockNotifyTaskStatusChanged).not.toHaveBeenCalled();
  });

  it("should not notify when newStatus is not in enabledStatuses", async () => {
    // Given
    const { default: NotificationListener } = await import("@/components/NotificationListener");
    render(
      <NotificationListener isNotificationEnabled={true} enabledStatuses={["progress", "review"]} />
    );
    const ws = MockWebSocket.instances[0];

    // When
    act(() => {
      ws.onmessage?.({
        data: JSON.stringify({
          type: "task-status-changed",
          projectName: "kanvibe",
          branchName: "feat/test",
          taskTitle: "테스트",
          description: null,
          newStatus: "pending",
        }),
      } as MessageEvent);
    });

    // Then
    expect(mockNotifyTaskStatusChanged).not.toHaveBeenCalled();
  });

  it("should render nothing", async () => {
    // Given
    const { default: NotificationListener } = await import("@/components/NotificationListener");

    // When
    const { container } = render(<NotificationListener {...defaultProps} />);

    // Then
    expect(container.innerHTML).toBe("");
  });
});
