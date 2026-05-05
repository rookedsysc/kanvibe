"use client";

import type { KanbanTask } from "@/entities/KanbanTask";

interface ProjectTaskGroupProps {
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

/** 프로젝트 그룹은 DnD 인덱스 구획만 유지하고 시각적 박스는 만들지 않는다 */
export default function ProjectTaskGroup({ children }: ProjectTaskGroupProps) {
  return <div className="mb-1.5">{children}</div>;
}
