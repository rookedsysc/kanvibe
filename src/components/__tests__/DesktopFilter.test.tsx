import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import DesktopFilter from "../DesktopFilter";

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string) => key,
}));

describe("DesktopFilter", () => {
  it("체크 표시가 아닌 행의 이름을 눌러도 선택이 바뀌고 메뉴가 열린 채 남는다", async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(<DesktopFilter hosts={["devbox"]} selectedDesktops={[]} onChange={onChange} />);

    await user.click(screen.getByRole("button", { name: "desktopFilter" }));
    await user.click(screen.getByText("devbox"));

    expect(onChange).toHaveBeenCalledWith(["ssh:devbox"]);
    expect(screen.getByRole("dialog", { name: "desktopFilter" })).toBeTruthy();
  });
});
