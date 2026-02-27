import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, act, waitFor } from "@testing-library/react";

// --- Mocks ---

const mockGetNotificationSettings = vi.fn().mockResolvedValue({
  isEnabled: true,
  enabledStatuses: ["todo", "progress", "pending", "review", "done"],
});
const mockGetWsPort = vi.fn().mockResolvedValue(12345);

vi.mock("@/lib/ipc", () => ({
  ipcSettings: {
    getNotificationSettings: (...args: unknown[]) => mockGetNotificationSettings(...args),
  },
  ipcApp: {
    getWsPort: (...args: unknown[]) => mockGetWsPort(...args),
  },
}));

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

/** 비동기 IPC 호출(getWsPort)이 완료되어 WebSocket이 생성될 때까지 대기한다 */
async function waitForWebSocket() {
  await waitFor(() => {
    expect(MockWebSocket.instances.length).toBeGreaterThan(0);
  });
}

describe("NotificationListener", () => {
  beforeEach(() => {
    vi.resetModules();
    mockNotifyTaskStatusChanged.mockClear();
    mockGetNotificationSettings.mockResolvedValue({
      isEnabled: true,
      enabledStatuses: ["todo", "progress", "pending", "review", "done"],
    });
    mockGetWsPort.mockResolvedValue(12345);
    MockWebSocket.instances = [];
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("should call notifyTaskStatusChanged when receiving task-status-changed message", async () => {
    // Given
    const { default: NotificationListener } = await import("@/components/NotificationListener");
    render(<NotificationListener />);
    await waitForWebSocket();
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
    render(<NotificationListener />);
    await waitForWebSocket();
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
    await act(async () => {
      render(<NotificationListener />);
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(MockWebSocket.instances).toHaveLength(1);

    // When
    act(() => {
      MockWebSocket.instances[0].onclose?.();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });

    // Then
    expect(MockWebSocket.instances).toHaveLength(2);

    vi.useRealTimers();
  });

  it("should close WebSocket connection on unmount", async () => {
    // Given
    const { default: NotificationListener } = await import("@/components/NotificationListener");
    const { unmount } = render(<NotificationListener />);
    await waitForWebSocket();
    const ws = MockWebSocket.instances[0];

    // When
    unmount();

    // Then
    expect(ws.close).toHaveBeenCalled();
  });

  it("should not notify when isNotificationEnabled is false", async () => {
    // Given
    mockGetNotificationSettings.mockResolvedValue({
      isEnabled: false,
      enabledStatuses: ["progress", "review"],
    });
    const { default: NotificationListener } = await import("@/components/NotificationListener");
    render(<NotificationListener />);
    await waitForWebSocket();
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
    mockGetNotificationSettings.mockResolvedValue({
      isEnabled: true,
      enabledStatuses: ["progress", "review"],
    });
    const { default: NotificationListener } = await import("@/components/NotificationListener");
    render(<NotificationListener />);
    await waitForWebSocket();
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
    const { container } = render(<NotificationListener />);

    // Then
    expect(container.innerHTML).toBe("");
  });
});
