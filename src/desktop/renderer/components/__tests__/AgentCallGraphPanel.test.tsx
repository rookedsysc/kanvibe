import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { AgentCallGraphPanel } from "@/desktop/renderer/components/AgentCallGraphPanel";
import type { AgentCallGraph, AgentCallNode, LiveAiSession } from "@/lib/aiSessions/types";

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, unknown>) => (
    values ? `${key}:${JSON.stringify(values)}` : key
  ),
}));

const session: LiveAiSession = {
  provider: "claude",
  sessionId: "session-a",
  currentTask: "실행중 AI 세션 패널",
  state: "running",
  lastActiveAt: "2026-08-15T00:12:00.000Z",
  runningSubtasks: [],
  terminalWindow: null,
};

function createNode(overrides: Partial<AgentCallNode> & { id: string }): AgentCallNode {
  return {
    agentType: null,
    skill: null,
    task: null,
    startedAt: null,
    endedAt: null,
    children: [],
    ...overrides,
  };
}

/**
 * 12분짜리 세션. 실행중인 `판정 로직 리뷰`가 밑에 하나를 더 띄웠고, `테스트 작성`은 따로 끝났다.
 * 시작과 끝이 어긋나 있어 병렬·순차가 막대로 갈린다.
 */
function createGraph(roots: AgentCallNode[]): AgentCallGraph {
  return {
    provider: "claude",
    sessionId: "session-a",
    startedAt: "2026-08-15T00:00:00.000Z",
    readAt: "2026-08-15T00:12:00.000Z",
    roots,
  };
}

const NESTED_ROOTS = [
  createNode({
    id: "a1",
    agentType: "general-purpose",
    task: "판정 로직 리뷰",
    startedAt: "2026-08-15T00:05:00.000Z",
    children: [createNode({
      id: "a2",
      agentType: "Explore",
      task: "머지 이력 조회",
      startedAt: "2026-08-15T00:08:00.000Z",
      endedAt: "2026-08-15T00:10:00.000Z",
    })],
  }),
  createNode({
    id: "a3",
    agentType: "Explore",
    task: "테스트 작성",
    startedAt: "2026-08-15T00:01:00.000Z",
    endedAt: "2026-08-15T00:03:00.000Z",
  }),
];

describe("AgentCallGraphPanel", () => {
  it("서브에이전트가 띄운 서브에이전트까지 계보 순서대로 편다", () => {
    render(<AgentCallGraphPanel session={session} graph={createGraph(NESTED_ROOTS)} onBack={vi.fn()} />);

    expect(screen.getAllByTestId("agent-call-node").map((node) => node.textContent)).toEqual([
      "general-purpose판정 로직 리뷰",
      "Explore머지 이력 조회",
      "Explore테스트 작성",
    ]);
  });

  it("깊이만큼 계보 선을 두고 막내에서 세로줄을 끊는다", () => {
    render(<AgentCallGraphPanel session={session} graph={createGraph(NESTED_ROOTS)} onBack={vi.fn()} />);

    const guidesByRow = screen.getAllByTestId("agent-call-node").map((node) =>
      [...node.querySelectorAll(".kv-graph-guide")].map((guide) => guide.getAttribute("data-guide")));

    expect(guidesByRow).toEqual([["tee"], ["pipe", "elbow"], ["elbow"]]);
  });

  it("막대를 같은 시간축 위에 놓고 실행중인 것만 지금까지 늘인다", () => {
    render(<AgentCallGraphPanel session={session} graph={createGraph(NESTED_ROOTS)} onBack={vi.fn()} />);

    const bars = screen.getAllByTestId("agent-call-bar");
    expect(bars.map((bar) => bar.getAttribute("data-state"))).toEqual(["running", "done", "done"]);

    // 12분 중 5분 뒤에 시작해 아직 도는 막대라 42%에서 끝까지 이어진다
    expect(Math.round(Number.parseFloat(bars[0].style.left))).toBe(42);
    expect(Math.round(Number.parseFloat(bars[0].style.width))).toBe(58);
  });

  it("시작 시각을 못 읽은 노드는 막대를 그리지 않는다", () => {
    render(<AgentCallGraphPanel
      session={session}
      graph={createGraph([createNode({ id: "a1", task: "시작 시각 없음" })])}
      onBack={vi.fn()}
    />);

    expect(screen.getByTestId("agent-call-node")).toBeTruthy();
    expect(screen.queryByTestId("agent-call-bar")).toBeNull();
  });

  it("서브에이전트·실행중 개수·깊이를 머리말에 센다", () => {
    render(<AgentCallGraphPanel session={session} graph={createGraph(NESTED_ROOTS)} onBack={vi.fn()} />);

    expect(screen.getByText('graph.summary:{"total":3,"running":1,"depth":2}')).toBeTruthy();
  });

  it("서브에이전트를 띄우지 않았으면 빈 안내를 보여준다", () => {
    render(<AgentCallGraphPanel session={session} graph={createGraph([])} onBack={vi.fn()} />);

    expect(screen.getByText("graph.empty")).toBeTruthy();
  });

  it("그래프를 아직 못 만들었으면 만드는 중임을 알린다", () => {
    render(<AgentCallGraphPanel session={session} graph={null} onBack={vi.fn()} />);

    expect(screen.getByText("graph.loading")).toBeTruthy();
  });

  it("머리말의 뒤로 버튼으로 목록에 돌아간다", () => {
    const onBack = vi.fn();
    render(<AgentCallGraphPanel session={session} graph={createGraph(NESTED_ROOTS)} onBack={onBack} />);

    fireEvent.click(screen.getByText("graph.back"));
    expect(onBack).toHaveBeenCalledTimes(1);
  });
});
