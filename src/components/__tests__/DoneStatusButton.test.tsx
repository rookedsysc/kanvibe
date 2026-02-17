import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

// --- Mocks ---

vi.mock("@/app/actions/appSettings", () => ({
  dismissDoneAlert: vi.fn().mockResolvedValue(undefined),
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

import DoneStatusButton from "../DoneStatusButton";

describe("DoneStatusButton", () => {
  const mockAction = vi.fn().mockResolvedValue(undefined);

  const defaultProps = {
    statusChangeAction: mockAction,
    label: "Done",
    hasCleanableResources: true,
    doneAlertDismissed: false,
  };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("should render the button with provided label", () => {
    // Given & When
    render(<DoneStatusButton {...defaultProps} />);

    // Then
    expect(screen.getByText("Done")).toBeTruthy();
  });

  it("should show confirm dialog when clicked with cleanable resources and not dismissed", async () => {
    // Given
    const user = userEvent.setup();
    render(<DoneStatusButton {...defaultProps} />);

    // When
    await user.click(screen.getByText("Done"));

    // Then
    expect(screen.getByText("Move to Done")).toBeTruthy();
    expect(screen.getByText("Moving to Done will delete resources.")).toBeTruthy();
  });

  it("should not show confirm dialog when doneAlertDismissed is true", async () => {
    // Given
    const user = userEvent.setup();
    render(<DoneStatusButton {...defaultProps} doneAlertDismissed={true} />);

    // When
    await user.click(screen.getByText("Done"));

    // Then
    expect(screen.queryByText("Move to Done")).toBeNull();
  });

  it("should not show confirm dialog when hasCleanableResources is false", async () => {
    // Given
    const user = userEvent.setup();
    render(<DoneStatusButton {...defaultProps} hasCleanableResources={false} />);

    // When
    await user.click(screen.getByText("Done"));

    // Then
    expect(screen.queryByText("Move to Done")).toBeNull();
  });

  it("should close dialog and submit form when confirmed", async () => {
    // Given
    const user = userEvent.setup();
    render(<DoneStatusButton {...defaultProps} />);
    await user.click(screen.getByText("Done"));
    expect(screen.getByText("Move to Done")).toBeTruthy();

    // When
    await user.click(screen.getByText("Move"));

    // Then
    expect(screen.queryByText("Move to Done")).toBeNull();
  });

  it("should close dialog without submitting when cancelled", async () => {
    // Given
    const user = userEvent.setup();
    render(<DoneStatusButton {...defaultProps} />);
    await user.click(screen.getByText("Done"));
    expect(screen.getByText("Move to Done")).toBeTruthy();

    // When
    await user.click(screen.getByText("Cancel"));

    // Then
    expect(screen.queryByText("Move to Done")).toBeNull();
  });

  it("should include hidden status input with value 'done'", () => {
    // Given & When
    render(<DoneStatusButton {...defaultProps} />);

    // Then
    const hiddenInput = document.querySelector("input[type='hidden'][name='status']") as HTMLInputElement;
    expect(hiddenInput).toBeTruthy();
    expect(hiddenInput.value).toBe("done");
  });
});
