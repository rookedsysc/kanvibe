import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

// --- Mocks ---

const mockDismissDoneAlert = vi.fn().mockResolvedValue(undefined);

vi.mock("@/app/actions/appSettings", () => ({
  dismissDoneAlert: (...args: unknown[]) => mockDismissDoneAlert(...args),
}));

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string) => {
    const messages: Record<string, string> = {
      title: "Move to Done",
      message: "Moving to Done will delete resources.",
      dontAskAgain: "Don't ask again",
      confirm: "Move",
      cancel: "Cancel",
    };
    return messages[key] ?? key;
  },
}));

import DoneConfirmDialog from "../DoneConfirmDialog";

describe("DoneConfirmDialog", () => {
  const defaultProps = {
    isOpen: true,
    onConfirm: vi.fn(),
    onCancel: vi.fn(),
  };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("should render nothing when isOpen is false", () => {
    // Given
    const props = { ...defaultProps, isOpen: false };

    // When
    const { container } = render(<DoneConfirmDialog {...props} />);

    // Then
    expect(container.innerHTML).toBe("");
  });

  it("should render title, message, checkbox and buttons when open", () => {
    // Given & When
    render(<DoneConfirmDialog {...defaultProps} />);

    // Then
    expect(screen.getByText("Move to Done")).toBeTruthy();
    expect(screen.getByText("Moving to Done will delete resources.")).toBeTruthy();
    expect(screen.getByText("Don't ask again")).toBeTruthy();
    expect(screen.getByText("Move")).toBeTruthy();
    expect(screen.getByText("Cancel")).toBeTruthy();
  });

  it("should call onCancel when cancel button is clicked", async () => {
    // Given
    const user = userEvent.setup();
    render(<DoneConfirmDialog {...defaultProps} />);

    // When
    await user.click(screen.getByText("Cancel"));

    // Then
    expect(defaultProps.onCancel).toHaveBeenCalledTimes(1);
    expect(defaultProps.onConfirm).not.toHaveBeenCalled();
  });

  it("should call onConfirm without dismissDoneAlert when checkbox is unchecked", async () => {
    // Given
    const user = userEvent.setup();
    render(<DoneConfirmDialog {...defaultProps} />);

    // When
    await user.click(screen.getByText("Move"));

    // Then
    expect(defaultProps.onConfirm).toHaveBeenCalledTimes(1);
    expect(mockDismissDoneAlert).not.toHaveBeenCalled();
  });

  it("should call dismissDoneAlert then onConfirm when checkbox is checked", async () => {
    // Given
    const user = userEvent.setup();
    render(<DoneConfirmDialog {...defaultProps} />);

    // When
    await user.click(screen.getByRole("checkbox"));
    await user.click(screen.getByText("Move"));

    // Then
    expect(mockDismissDoneAlert).toHaveBeenCalledTimes(1);
    expect(defaultProps.onConfirm).toHaveBeenCalledTimes(1);
  });
});
