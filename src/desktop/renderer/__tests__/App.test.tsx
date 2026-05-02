import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "@/desktop/renderer/App";
import { INITIAL_DESKTOP_LOAD_TIMEOUT_MS } from "@/desktop/renderer/utils/loadingTimeout";

const mocks = vi.hoisted(() => ({
  getSessionState: vi.fn(),
}));

vi.mock("@/desktop/renderer/actions/auth", () => ({
  getSessionState: (...args: unknown[]) => mocks.getSessionState(...args),
}));

vi.mock("@/components/LoginForm", () => ({
  default: () => <div>login form</div>,
}));

vi.mock("@/desktop/renderer/routes/BoardRoute", () => ({
  default: () => <div>board route</div>,
}));

vi.mock("@/desktop/renderer/routes/DiffRoute", () => ({
  default: () => <div>diff route</div>,
}));

vi.mock("@/desktop/renderer/routes/PaneLayoutRoute", () => ({
  default: () => <div>pane layout route</div>,
}));

vi.mock("@/desktop/renderer/routes/TaskDetailRoute", () => ({
  default: () => <div>task detail route</div>,
}));

vi.mock("@/desktop/renderer/routes/NotFoundRoute", () => ({
  default: () => <div>not found route</div>,
}));

vi.mock("@/desktop/renderer/components/TaskQuickSearchDialog", () => ({
  default: () => <div>quick search</div>,
}));

vi.mock("@/desktop/renderer/components/NotificationListener", () => ({
  default: () => <div>notification listener</div>,
}));

vi.mock("@/desktop/renderer/components/BoardEventAlert", () => ({
  default: () => <div>board event alert</div>,
}));

describe("App", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.location.hash = "#/ko/login";
    window.kanvibeDesktop = {
      isDesktop: true,
      onBoardEvent: vi.fn(() => vi.fn()),
    } as unknown as NonNullable<typeof window.kanvibeDesktop>;
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("session lookup failure does not keep the app on the loading screen", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.getSessionState.mockRejectedValue(new Error("ipc failed"));

    try {
      render(<App />);

      await waitFor(() => {
        expect(screen.queryByText("Loading...")).toBeNull();
      });
      expect(screen.getByText("login form")).toBeTruthy();
    } finally {
      consoleError.mockRestore();
    }
  });

  it("should not stay on the loading screen when session lookup never settles", async () => {
    // Given
    vi.useFakeTimers();
    mocks.getSessionState.mockReturnValue(new Promise(() => {}));

    // When
    render(<App />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(INITIAL_DESKTOP_LOAD_TIMEOUT_MS);
    });

    // Then
    expect(screen.queryByText("Loading...")).toBeNull();
    expect(screen.getByText("login form")).toBeTruthy();
  });
});
