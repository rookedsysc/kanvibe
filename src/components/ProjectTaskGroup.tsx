"use client";

import type { KanbanTask } from "@/entities/KanbanTask";

interface ProjectTaskGroupProps {
  projectName: string;
  color: string;
  hasBranchRelations: boolean;
  /** 그룹 내에 기본 브랜치가 없는 자식 전용 그룹 */
  isChildGroup?: boolean;
  children: React.ReactNode;
}

export interface BranchTreeNode {
  depth: number;
  isLast: boolean;
  hasChildren: boolean;
}

/** baseBranch → branchName 관계를 분석하여 태스크를 트리 순서로 정렬하고 각 노드의 트리 정보를 반환한다 */
export function buildBranchTree(tasks: KanbanTask[]): { sorted: KanbanTask[]; treeInfo: Map<string, BranchTreeNode> } {
  const treeInfo = new Map<string, BranchTreeNode>();

  /** branchName → task 역매핑 */
  const branchToTask = new Map<string, KanbanTask>();
  for (const task of tasks) {
    if (task.branchName) {
      branchToTask.set(task.branchName, task);
    }
  }

  /** 부모 → 자식 태스크 매핑 */
  const childrenMap = new Map<string, KanbanTask[]>();
  const hasParent = new Set<string>();

  for (const task of tasks) {
    if (task.baseBranch && branchToTask.has(task.baseBranch)) {
      const parent = branchToTask.get(task.baseBranch)!;
      if (parent.id === task.id) continue;
      if (!childrenMap.has(parent.id)) childrenMap.set(parent.id, []);
      childrenMap.get(parent.id)!.push(task);
      hasParent.add(task.id);
    }
  }

  /** 루트 노드: 부모가 없는 태스크 */
  const roots = tasks.filter((t) => !hasParent.has(t.id));

  /** DFS 순회로 정렬된 배열과 트리 메타정보 생성 */
  const sorted: KanbanTask[] = [];

  function traverse(task: KanbanTask, depth: number, isLast: boolean) {
    const children = childrenMap.get(task.id) || [];
    treeInfo.set(task.id, { depth, isLast, hasChildren: children.length > 0 });
    sorted.push(task);
    children.forEach((child, i) => {
      traverse(child, depth + 1, i === children.length - 1);
    });
  }

  roots.forEach((root, i) => {
    traverse(root, 0, i === roots.length - 1);
  });

  return { sorted, treeInfo };
}

/** hex 색상에서 10% 밝기의 배경색을 생성한다 */
function hexToBg(hex: string): string {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  const mix = (c: number) => Math.round(c + (255 - c) * 0.88);
  return `#${mix(r).toString(16).padStart(2, "0")}${mix(g).toString(16).padStart(2, "0")}${mix(b).toString(16).padStart(2, "0")}`;
}

/** 프로젝트 그룹 래퍼: 파스텔 테두리 + 프로젝트명 라벨. 자식 전용 그룹은 점선 테두리 + 배경 없음 */
export default function ProjectTaskGroup({ projectName, color, hasBranchRelations, isChildGroup, children }: ProjectTaskGroupProps) {
  return (
    <div
      className={`rounded-lg mb-3 border-2 ${isChildGroup ? "border-dashed" : ""}`}
      style={{
        borderColor: color,
        backgroundColor: isChildGroup ? undefined : hexToBg(color),
      }}
    >
      {/* 프로젝트명 라벨 */}
      <div className="px-3 py-1.5 flex items-center gap-1.5">
        <span className="text-xs font-semibold text-text-secondary truncate">
          {projectName}
        </span>
        {hasBranchRelations && (
          <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor" className="text-text-muted shrink-0" aria-label="Branch flow">
            <path d="M9.5 3.25a2.25 2.25 0 1 1 3 2.122V6A2.5 2.5 0 0 1 10 8.5H6a1 1 0 0 0-1 1v1.128a2.251 2.251 0 1 1-1.5 0V5.372a2.25 2.25 0 1 1 1.5 0v1.836A2.492 2.492 0 0 1 6 7h4a1 1 0 0 0 1-1v-.628A2.25 2.25 0 0 1 9.5 3.25zM4.25 12a.75.75 0 1 0 0 1.5.75.75 0 0 0 0-1.5zM3.5 3.25a.75.75 0 1 0 1.5 0 .75.75 0 0 0-1.5 0zm8 0a.75.75 0 1 0 1.5 0 .75.75 0 0 0-1.5 0z"/>
          </svg>
        )}
      </div>

      {/* 태스크 카드 영역 */}
      <div className="px-1 pb-1">
        {children}
      </div>
    </div>
  );
}
