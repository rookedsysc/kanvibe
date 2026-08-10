import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { LiveAiSessionPanel } from "@/desktop/renderer/components/LiveAiSessionPanel";
import type { LiveAiSession } from "@/lib/aiSessions/types";

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, unknown>) => (
    values ? `${key}:${JSON.stringify(values)}` : key
  ),
}));

function createSession(overrides: Partial<LiveAiSession> = {}): LiveAiSession {
  return {
    provider: "claude",
    sessionId: "session-a",
    currentTask: null,
    state: "running",
    lastActiveAt: "2026-08-10T00:00:00.000Z",
    runningSubtasks: [],
    terminalWindow: null,
    ...overrides,
  };
}

describe("LiveAiSessionPanel", () => {
  it("실행중 세션과 유휴 세션을 다른 상태 점으로 구분한다", () => {
    render(<LiveAiSessionPanel sessions={[
      createSession(),
      createSession({ provider: "codex", state: "idle" }),
    ]} />);

    expect(screen.getByTestId("live-ai-session-state-running")).toBeTruthy();
    expect(screen.getByTestId("live-ai-session-state-idle")).toBeTruthy();
  });

  it("실행중 서브태스크의 개수와 이름을 보여준다", () => {
    render(<LiveAiSessionPanel sessions={[createSession({
      runningSubtasks: [
        { id: "agent-1", name: "Explore", lastActiveAt: null },
        { id: "agent-2", name: null, lastActiveAt: null },
      ],
    })]} />);

    expect(screen.getByText('subtaskCount:{"count":2}')).toBeTruthy();
    expect(screen.getAllByTestId("live-ai-subtask").map((node) => node.textContent))
      .toEqual(["├─Explore", "└─agent-2"]);
  });

  it("터미널 창을 찾은 세션만 클릭할 수 있다", () => {
    const onSelectSession = vi.fn();
    render(<LiveAiSessionPanel
      sessions={[
        createSession({
          terminalWindow: { sessionName: "kanvibe-task", windowId: "@7", windowName: "claude" },
        }),
        createSession({ provider: "codex" }),
      ]}
      onSelectSession={onSelectSession}
    />);

    const buttons = screen.getAllByRole("button");
    expect(buttons).toHaveLength(1);

    fireEvent.click(buttons[0]);
    expect(onSelectSession).toHaveBeenCalledWith(expect.objectContaining({ provider: "claude" }));
  });

  it("세션이 지금 하고 있는 작업을 provider 이름 대신 보여준다", () => {
    render(<LiveAiSessionPanel sessions={[createSession({ currentTask: "실행중 세션 패널 구현" })]} />);

    expect(screen.getByText("실행중 세션 패널 구현")).toBeTruthy();
    expect(screen.queryByText("claude")).toBeNull();
  });

  it("작업을 읽지 못하면 provider 이름으로 되돌린다", () => {
    render(<LiveAiSessionPanel sessions={[createSession({ currentTask: null })]} />);

    expect(screen.getByText("claude")).toBeTruthy();
  });

  it("서브태스크를 세션에 매달린 가지로 그리고 마지막만 끝가지로 닫는다", () => {
    render(<LiveAiSessionPanel sessions={[createSession({
      runningSubtasks: [
        { id: "agent-1", name: "코드베이스 조사", lastActiveAt: null },
        { id: "agent-2", name: "판정 로직 리뷰", lastActiveAt: null },
        { id: "agent-3", name: "테스트 작성", lastActiveAt: null },
      ],
    })]} />);

    const branches = screen.getAllByTestId("live-ai-subtask")
      .map((node) => node.textContent);
    expect(branches).toEqual([
      "├─코드베이스 조사",
      "├─판정 로직 리뷰",
      "└─테스트 작성",
    ]);
  });

  it("서브태스크가 하나면 끝가지 하나로 닫는다", () => {
    render(<LiveAiSessionPanel sessions={[createSession({
      runningSubtasks: [{ id: "agent-1", name: "코드베이스 조사", lastActiveAt: null }],
    })]} />);

    expect(screen.getByTestId("live-ai-subtask").textContent).toBe("└─코드베이스 조사");
  });

  it("실행중 세션에만 진행 표시를 그린다", () => {
    render(<LiveAiSessionPanel sessions={[
      createSession(),
      createSession({ provider: "codex", state: "idle" }),
    ]} />);

    const progressBars = screen.getAllByTestId("live-ai-session-progress");
    expect(progressBars).toHaveLength(1);
    expect(progressBars[0].className).toContain("kv-live-progress");
  });

  it("실행중인 세션이 없으면 빈 안내를 보여준다", () => {
    render(<LiveAiSessionPanel sessions={[]} />);

    expect(screen.getByText("empty")).toBeTruthy();
  });
});
