import { ipcMain } from "electron";
import { getPaneLayoutConfigRepository } from "../database";
import { PaneLayoutConfig, PaneLayoutType, type PaneCommand } from "@/entities/PaneLayoutConfig";

/** TypeORM 엔티티를 직렬화 가능한 plain object로 변환한다 */
function serialize<T>(data: T): T {
  return JSON.parse(JSON.stringify(data));
}

/** Pane 레이아웃 관련 IPC 핸들러를 등록한다 */
export function registerPaneLayoutHandlers(): void {
  /** 글로벌 기본 pane 레이아웃을 조회한다 */
  ipcMain.handle("paneLayout:getGlobal", async () => {
    const repo = getPaneLayoutConfigRepository();
    const config = await repo.findOne({ where: { isGlobal: true } });
    return config ? serialize(config) : null;
  });

  /** 프로젝트별 pane 레이아웃을 조회한다 (fallback 없음) */
  ipcMain.handle("paneLayout:getProject", async (_event, projectId: string) => {
    const repo = getPaneLayoutConfigRepository();
    const config = await repo.findOne({ where: { projectId } });
    return config ? serialize(config) : null;
  });

  /** 프로젝트에 적용될 실제 pane 레이아웃을 조회한다 (프로젝트 → 글로벌 fallback) */
  ipcMain.handle("paneLayout:getEffective", async (_event, projectId?: string) => {
    const repo = getPaneLayoutConfigRepository();

    if (projectId) {
      const projectConfig = await repo.findOne({ where: { projectId } });
      if (projectConfig) return serialize(projectConfig);
    }

    const globalConfig = await repo.findOne({ where: { isGlobal: true } });
    return globalConfig ? serialize(globalConfig) : null;
  });

  /** 모든 pane 레이아웃 설정을 조회한다 (글로벌 + 프로젝트별) */
  ipcMain.handle("paneLayout:getAll", async () => {
    const repo = getPaneLayoutConfigRepository();
    const configs = await repo.find({ order: { isGlobal: "DESC", createdAt: "ASC" } });
    return serialize(configs);
  });

  /** pane 레이아웃을 저장한다 (upsert 패턴: 기존 설정이 있으면 업데이트, 없으면 생성) */
  ipcMain.handle(
    "paneLayout:save",
    async (
      _event,
      input: {
        layoutType: PaneLayoutType;
        panes: PaneCommand[];
        projectId?: string | null;
        isGlobal?: boolean;
      },
    ) => {
      const repo = getPaneLayoutConfigRepository();

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
        return serialize(saved);
      }

      const config = repo.create({
        layoutType: input.layoutType,
        panes: input.panes,
        projectId: input.projectId || null,
        isGlobal: input.isGlobal ?? false,
      });

      const saved = await repo.save(config);
      return serialize(saved);
    },
  );

  /** pane 레이아웃을 삭제한다 */
  ipcMain.handle("paneLayout:delete", async (_event, id: string) => {
    const repo = getPaneLayoutConfigRepository();
    const result = await repo.delete(id);
    return (result.affected ?? 0) > 0;
  });
}
