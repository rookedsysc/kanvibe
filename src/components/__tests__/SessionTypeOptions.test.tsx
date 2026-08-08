import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import SessionTypeOptions from "../SessionTypeOptions";

describe("SessionTypeOptions", () => {
  it("tmux·zellij·terminal 세 가지를 선택지로 보여 준다", () => {
    render(<select><SessionTypeOptions /></select>);

    expect(screen.getAllByRole("option").map((option) => option.getAttribute("value")))
      .toEqual(["tmux", "zellij", "terminal"]);
  });
});
