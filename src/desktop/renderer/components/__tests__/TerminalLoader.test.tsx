import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import TerminalLoader from "../TerminalLoader";

const { mockTerminalRender } = vi.hoisted(() => ({
  mockTerminalRender: vi.fn(),
}));

vi.mock("@/desktop/renderer/components/Terminal", () => ({
  default: ({ taskId }: { taskId: string }) => {
    mockTerminalRender(taskId);
    return <div data-testid="terminal">{taskId}</div>;
  },
}));

describe("TerminalLoader", () => {
  it("idle 대기 없이 즉시 터미널을 마운트한다", () => {
    render(<TerminalLoader taskId="task-1" />);

    expect(screen.getByTestId("terminal").textContent).toBe("task-1");
    expect(mockTerminalRender).toHaveBeenCalledWith("task-1");
  });
});
