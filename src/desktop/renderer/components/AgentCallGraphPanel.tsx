"use client";

import { useTranslations } from "next-intl";
import { AiProviderIcon } from "@/components/AiProviderIcon";
import { flattenAgentCallNodes } from "@/lib/aiSessions/agentCallGraph";
import type { AgentCallGraph, AgentCallNode, LiveAiSession } from "@/lib/aiSessions/types";

/** 한 행 왼쪽에 그리는 계보 선 조각 */
type AgentCallGuide = "pipe" | "gap" | "tee" | "elbow";

interface AgentCallRow {
  node: AgentCallNode;
  guides: AgentCallGuide[];
}

interface AgentCallGraphPanelProps {
  session: LiveAiSession;
  graph: AgentCallGraph | null;
  onBack: () => void;
  className?: string;
}

/**
 * 세션 하나가 어떻게 갈라졌는지를 보여준다.
 *
 * 왼쪽은 계보, 오른쪽은 모든 행이 공유하는 하나의 시간축이다.
 * 트리만으로는 병렬인지 순차인지 알 수 없어서, 막대가 겹치는지로 그 답을 대신한다.
 * 끝난 가지도 지우지 않는다. 지우면 왜 오래 걸렸는지를 물을 수 없다.
 */
export function AgentCallGraphPanel({
  session,
  graph,
  onBack,
  className,
}: AgentCallGraphPanelProps) {
  const t = useTranslations("taskDetail.liveSessions");
  const nodes = graph ? flattenAgentCallNodes(graph.roots) : [];

  return (
    <div className={className} data-testid="agent-call-graph-panel">
      <div className="flex items-center gap-2 border-b border-border-subtle px-2 py-1.5">
        <button
          type="button"
          className="shrink-0 rounded px-1.5 py-0.5 text-[11px] text-text-secondary hover:bg-bg-page"
          onClick={onBack}
        >
          {t("graph.back")}
        </button>
        <AiProviderIcon provider={session.provider} size={14} />
        <span className="min-w-0 flex-1 truncate text-xs text-text-primary">
          {session.currentTask ?? session.provider}
        </span>
        <span className="shrink-0 tabular-nums text-[10px] text-text-muted">
          {t("graph.summary", {
            total: nodes.length,
            running: nodes.filter((node) => node.endedAt === null).length,
            depth: graph ? measureGraphDepth(graph.roots) : 0,
          })}
        </span>
      </div>

      {!graph ? (
        <p className="px-2 py-3 text-xs text-text-muted">{t("graph.loading")}</p>
      ) : graph.roots.length === 0 ? (
        <p className="px-2 py-3 text-xs text-text-muted">{t("graph.empty")}</p>
      ) : (
        <AgentCallTimeline graph={graph} />
      )}
    </div>
  );
}

function AgentCallTimeline({ graph }: { graph: AgentCallGraph }) {
  const t = useTranslations("taskDetail.liveSessions");
  const timeline = toGraphTimeline(graph);

  return (
    <div className="px-2 py-1.5">
      <div className="flex items-center gap-3 pb-1">
        <span className="min-w-0 flex-1 text-[9px] uppercase tracking-wider text-text-muted">
          {t("graph.lineage")}
        </span>
        <span className="flex w-[168px] shrink-0 justify-between text-[9px] tabular-nums text-text-muted">
          <span>{t("graph.start")}</span>
          <span>{t("graph.now")}</span>
        </span>
      </div>

      {toAgentCallRows(graph.roots).map(({ node, guides }) => (
        <div key={node.id} className="flex min-h-[24px] items-center gap-3" data-testid="agent-call-node">
          <span className="flex min-w-0 flex-1 items-stretch">
            {guides.map((guide, guideIndex) => (
              <span
                key={guideIndex}
                aria-hidden="true"
                className="kv-graph-guide"
                data-guide={guide}
              />
            ))}
            <span className="flex min-w-0 flex-1 items-center gap-1.5">
              {node.agentType && (
                <span className="shrink-0 rounded border border-border-subtle px-1 text-[9px] text-text-secondary">
                  {node.agentType}
                </span>
              )}
              {node.skill && (
                <span className="shrink-0 rounded border border-dashed border-border-subtle px-1 text-[9px] text-text-muted">
                  {node.skill}
                </span>
              )}
              <span
                className={`min-w-0 flex-1 truncate text-[11px] ${
                  node.endedAt === null ? "text-text-primary" : "text-text-secondary"
                }`}
                title={node.task ?? node.id}
              >
                {node.task ?? node.id}
              </span>
            </span>
          </span>

          <span className="kv-graph-track h-4 w-[168px] shrink-0">
            <AgentCallBar node={node} timeline={timeline} />
          </span>
        </div>
      ))}
    </div>
  );
}

/** 모든 막대가 같은 자를 쓰도록 그래프 전체의 시작과 끝을 한 번만 계산한다 */
interface GraphTimeline {
  startMs: number;
  spanMs: number;
  readAtMs: number;
}

function toGraphTimeline(graph: AgentCallGraph): GraphTimeline {
  const readAtMs = Date.parse(graph.readAt);
  const startMs = graph.startedAt === null ? readAtMs : Date.parse(graph.startedAt);

  return { startMs, spanMs: Math.max(readAtMs - startMs, 1), readAtMs };
}

/** 시작 시각을 못 읽은 노드는 자리를 지어내지 않고 막대를 그리지 않는다 */
function AgentCallBar({ node, timeline }: { node: AgentCallNode; timeline: GraphTimeline }) {
  const t = useTranslations("taskDetail.liveSessions");

  if (node.startedAt === null) {
    return null;
  }

  const startedAtMs = Date.parse(node.startedAt);
  if (!Number.isFinite(startedAtMs)) {
    return null;
  }

  const isRunning = node.endedAt === null;
  const endedAtMs = isRunning ? timeline.readAtMs : Date.parse(node.endedAt as string);
  const leftPercent = toBoundedPercent((startedAtMs - timeline.startMs) / timeline.spanMs);
  const rightPercent = toBoundedPercent((endedAtMs - timeline.startMs) / timeline.spanMs);

  return (
    <span
      aria-label={isRunning ? t("running") : t("idle")}
      className="kv-graph-bar"
      data-state={isRunning ? "running" : "done"}
      data-testid="agent-call-bar"
      style={{ left: `${leftPercent}%`, width: `${Math.max(rightPercent - leftPercent, 1)}%` }}
    />
  );
}

function toBoundedPercent(ratio: number): number {
  return Number.isFinite(ratio) ? Math.min(100, Math.max(0, ratio * 100)) : 0;
}

/**
 * 트리를 화면 순서대로 펴면서 각 행이 그릴 선 조각을 정한다.
 * 조상이 막내였으면 그 세로줄은 이미 끝났으므로 빈칸을 두고, 아니면 계속 내려오는 세로줄을 그린다.
 */
function toAgentCallRows(nodes: AgentCallNode[], ancestorGuides: AgentCallGuide[] = []): AgentCallRow[] {
  return nodes.flatMap((node, nodeIndex) => {
    const isLastSibling = nodeIndex === nodes.length - 1;

    return [
      { node, guides: [...ancestorGuides, isLastSibling ? "elbow" as const : "tee" as const] },
      ...toAgentCallRows(node.children, [...ancestorGuides, isLastSibling ? "gap" : "pipe"]),
    ];
  });
}

function measureGraphDepth(nodes: AgentCallNode[]): number {
  return nodes.reduce((deepest, node) => Math.max(deepest, 1 + measureGraphDepth(node.children)), 0);
}
