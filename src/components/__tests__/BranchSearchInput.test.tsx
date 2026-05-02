import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import BranchSearchInput from "../BranchSearchInput";

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, unknown>) => {
    if (!values) return key;
    return `${key}:${JSON.stringify(values)}`;
  },
}));

describe("BranchSearchInput", () => {
  it("드롭다운이 열린 상태에서도 Tab은 다음 입력창으로 이동한다", async () => {
    // Given
    const user = userEvent.setup();
    render(
      <>
        <BranchSearchInput
          branches={["main", "develop"]}
          value="main"
          onChange={vi.fn()}
        />
        <input aria-label="branch name" />
      </>,
    );

    // When
    await user.click(screen.getByDisplayValue("main"));
    expect(screen.getByRole("button", { name: "main" })).toBeTruthy();
    await user.tab();

    // Then
    expect(document.activeElement).toBe(screen.getByLabelText("branch name"));
  });
});
