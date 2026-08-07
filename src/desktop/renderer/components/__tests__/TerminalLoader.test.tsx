import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import TerminalLoader from "../TerminalLoader";
import type { TerminalTab } from "@/desktop/shared/terminalTabs";

const { mockTerminalRender } = vi.hoisted(() => ({
  mockTerminalRender: vi.fn(),
}));

vi.mock("@/desktop/renderer/components/Terminal", () => ({
  default: ({ taskId, tabId, isHidden }: { taskId: string; tabId?: string | null; isHidden?: boolean }) => {
    mockTerminalRender(taskId, tabId ?? null, isHidden ?? false);
    return <div data-testid={`terminal-${tabId ?? "single"}`}>{taskId}</div>;
  },
}));

const TABS: TerminalTab[] = [
  { id: "t1", nativeIndex: 0, name: "shell", isActive: false },
  { id: "t2", nativeIndex: 1, name: "logs", isActive: true },
];

describe("TerminalLoader", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("idle 대기 없이 즉시 터미널을 마운트한다", () => {
    render(<TerminalLoader taskId="task-1" />);

    expect(screen.getByTestId("terminal-single").textContent).toBe("task-1");
    expect(mockTerminalRender).toHaveBeenCalledWith("task-1", null, false);
  });

  it("탭 목록이 아직 안 왔으면 버려질 PTY를 만들지 않는다", () => {
    render(<TerminalLoader taskId="task-1" tabs={[]} />);

    expect(screen.queryAllByTestId(/^terminal-/)).toHaveLength(0);
    expect(mockTerminalRender).not.toHaveBeenCalled();
  });

  it("탭마다 터미널을 만들고 비활성 탭도 마운트한 채 숨긴다", () => {
    render(<TerminalLoader taskId="task-1" tabs={TABS} />);

    expect(screen.getByTestId("terminal-t1")).toBeTruthy();
    expect(screen.getByTestId("terminal-t2")).toBeTruthy();
    expect(mockTerminalRender).toHaveBeenCalledWith("task-1", "t1", true);
    expect(mockTerminalRender).toHaveBeenCalledWith("task-1", "t2", false);
  });

  it("비활성 탭 컨테이너만 hidden 클래스를 갖는다", () => {
    render(<TerminalLoader taskId="task-1" tabs={TABS} />);

    expect(screen.getByTestId("terminal-t1").parentElement?.className).toContain("hidden");
    expect(screen.getByTestId("terminal-t2").parentElement?.className).not.toContain("hidden");
  });
});
