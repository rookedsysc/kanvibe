

import type { Project } from "./Project";
export enum PaneLayoutType {
  SINGLE = "single",
  HORIZONTAL_2 = "horizontal_2",
  VERTICAL_2 = "vertical_2",
  LEFT_RIGHT_TB = "left_right_tb",
  LEFT_TB_RIGHT = "left_tb_right",
  QUAD = "quad",
}
export interface PaneCommand {
  position: number;
  command: string;
}
export interface PaneLayoutConfig {
id: string;
layoutType: PaneLayoutType;
panes: PaneCommand[];
project: Project | null;
projectId: string | null;
isGlobal: boolean;
createdAt: Date;
updatedAt: Date;
}