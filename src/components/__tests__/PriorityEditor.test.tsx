import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, act } from "@testing-library/react";
import PriorityEditor from "../PriorityEditor";
import { TaskPriority } from "@/entities/TaskPriority";

const mockRefresh = vi.fn();
const mockUpdateTask = vi.fn().mockResolvedValue({});

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

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string) => {
    const translations: Record<string, string> = {
      priorityNone: "없음",
      priorityLow: "!",
      priorityMedium: "!!",
      priorityHigh: "!!!",
    };
    return translations[key] ?? key;
  },
}));

vi.mock("@/app/actions/kanban", () => ({
  updateTask: (...args: unknown[]) => mockUpdateTask(...args),
}));

describe("PriorityEditor", () => {
  beforeEach(() => {
    mockRefresh.mockClear();
    mockUpdateTask.mockClear();
  });

  it("should render PrioritySelector with current priority", () => {
    // Given & When
    render(<PriorityEditor taskId="task-1" currentPriority={TaskPriority.HIGH} />);

    // Then
    const highButton = screen.getByText("!!!");
    expect(highButton.className).toContain("ring-2");
  });

  it("should call updateTask with new priority when selection changes", async () => {
    // Given
    render(<PriorityEditor taskId="task-1" currentPriority={null} />);

    // When
    await act(async () => {
      fireEvent.click(screen.getByText("!!"));
    });

    // Then
    expect(mockUpdateTask).toHaveBeenCalledWith("task-1", { priority: TaskPriority.MEDIUM });
  });

  it("should call updateTask with null when 없음 is selected", async () => {
    // Given
    render(<PriorityEditor taskId="task-1" currentPriority={TaskPriority.LOW} />);

    // When
    await act(async () => {
      fireEvent.click(screen.getByText("없음"));
    });

    // Then
    expect(mockUpdateTask).toHaveBeenCalledWith("task-1", { priority: null });
  });

  it("should call router.refresh() after updateTask completes", async () => {
    // Given
    render(<PriorityEditor taskId="task-1" currentPriority={null} />);

    // When
    await act(async () => {
      fireEvent.click(screen.getByText("!"));
    });

    // Then
    expect(mockUpdateTask).toHaveBeenCalled();
    expect(mockRefresh).toHaveBeenCalled();
  });
});
