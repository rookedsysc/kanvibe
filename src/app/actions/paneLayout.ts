"use server";

import { revalidatePath } from "next/cache";
import { getPaneLayoutConfigRepository } from "@/lib/database";
import { PaneLayoutConfig, PaneLayoutType, type PaneCommand } from "@/entities/PaneLayoutConfig";

/** TypeORM 엔티티를 직렬화 가능한 plain object로 변환한다 */
function serialize<T>(data: T): T {
  return JSON.parse(JSON.stringify(data));
}

/** 글로벌 기본 pane 레이아웃 조회 */
export async function getGlobalPaneLayout(): Promise<PaneLayoutConfig | null> {
  const repo = await getPaneLayoutConfigRepository();
  const config = await repo.findOne({ where: { isGlobal: true } });
  return config ? serialize(config) : null;
}

/** 프로젝트별 pane 레이아웃 조회 (프로젝트 설정만, fallback 없음) */
export async function getProjectPaneLayout(projectId: string): Promise<PaneLayoutConfig | null> {
  const repo = await getPaneLayoutConfigRepository();
  const config = await repo.findOne({ where: { projectId } });
  return config ? serialize(config) : null;
}

/** 프로젝트에 적용될 실제 pane 레이아웃 조회 (프로젝트 → 글로벌 fallback) */
export async function getEffectivePaneLayout(projectId?: string): Promise<PaneLayoutConfig | null> {
  if (projectId) {
    const projectConfig = await getProjectPaneLayout(projectId);
    if (projectConfig) return projectConfig;
  }
  return getGlobalPaneLayout();
}

/** 모든 pane 레이아웃 설정 조회 (글로벌 + 프로젝트별) */
export async function getAllPaneLayouts(): Promise<PaneLayoutConfig[]> {
  const repo = await getPaneLayoutConfigRepository();
  const configs = await repo.find({ order: { isGlobal: "DESC", createdAt: "ASC" } });
  return serialize(configs);
}

export interface SavePaneLayoutInput {
  layoutType: PaneLayoutType;
  panes: PaneCommand[];
  projectId?: string | null;
  isGlobal?: boolean;
}

/** pane 레이아웃 저장 (upsert 패턴: 기존 설정이 있으면 업데이트, 없으면 생성) */
export async function savePaneLayout(input: SavePaneLayoutInput): Promise<PaneLayoutConfig> {
  const repo = await getPaneLayoutConfigRepository();

  let existing: PaneLayoutConfig | null = null;
  if (input.isGlobal) {
    existing = await repo.findOne({ where: { isGlobal: true } });
  } else if (input.projectId) {
    existing = await repo.findOne({ where: { projectId: input.projectId } });
  }

  if (existing) {
    existing.layoutType = input.layoutType;
    existing.panes = input.panes;
    const saved = await repo.save(existing);
    revalidatePath("/[locale]/pane-layout", "page");
    return serialize(saved);
  }

  const config = repo.create({
    layoutType: input.layoutType,
    panes: input.panes,
    projectId: input.projectId || null,
    isGlobal: input.isGlobal ?? false,
  });

  const saved = await repo.save(config);
  revalidatePath("/[locale]/pane-layout", "page");
  return serialize(saved);
}

/** pane 레이아웃 삭제 */
export async function deletePaneLayout(id: string): Promise<boolean> {
  const repo = await getPaneLayoutConfigRepository();
  const result = await repo.delete(id);
  revalidatePath("/[locale]/pane-layout", "page");
  return (result.affected ?? 0) > 0;
}
