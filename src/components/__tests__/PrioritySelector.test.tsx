import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import PrioritySelector from "../PrioritySelector";
import { TaskPriority } from "@/entities/TaskPriority";

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

describe("PrioritySelector", () => {
  let onChange: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    onChange = vi.fn();
  });

  it("should render all 4 priority options", () => {
    // Given
    render(<PrioritySelector value={null} onChange={onChange} />);

    // When
    const buttons = screen.getAllByRole("button");

    // Then
    expect(buttons).toHaveLength(4);
    expect(buttons[0].textContent).toBe("없음");
    expect(buttons[1].textContent).toBe("!");
    expect(buttons[2].textContent).toBe("!!");
    expect(buttons[3].textContent).toBe("!!!");
  });

  it("should call onChange with TaskPriority.LOW when ! button is clicked", () => {
    // Given
    render(<PrioritySelector value={null} onChange={onChange} />);

    // When
    fireEvent.click(screen.getByText("!"));

    // Then
    expect(onChange).toHaveBeenCalledWith(TaskPriority.LOW);
  });

  it("should call onChange with TaskPriority.MEDIUM when !! button is clicked", () => {
    // Given
    render(<PrioritySelector value={null} onChange={onChange} />);

    // When
    fireEvent.click(screen.getByText("!!"));

    // Then
    expect(onChange).toHaveBeenCalledWith(TaskPriority.MEDIUM);
  });

  it("should call onChange with TaskPriority.HIGH when !!! button is clicked", () => {
    // Given
    render(<PrioritySelector value={null} onChange={onChange} />);

    // When
    fireEvent.click(screen.getByText("!!!"));

    // Then
    expect(onChange).toHaveBeenCalledWith(TaskPriority.HIGH);
  });

  it("should call onChange with null when 없음 button is clicked", () => {
    // Given
    render(<PrioritySelector value={TaskPriority.HIGH} onChange={onChange} />);

    // When
    fireEvent.click(screen.getByText("없음"));

    // Then
    expect(onChange).toHaveBeenCalledWith(null);
  });

  it("should apply ring style to selected option", () => {
    // Given & When
    render(<PrioritySelector value={TaskPriority.MEDIUM} onChange={onChange} />);

    // Then
    const mediumButton = screen.getByText("!!");
    expect(mediumButton.className).toContain("ring-2");

    const lowButton = screen.getByText("!");
    expect(lowButton.className).toContain("opacity-60");
  });
});
