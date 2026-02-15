import "reflect-metadata";
import {
  Entity,
  PrimaryGeneratedColumn,
  Column,
  CreateDateColumn,
  UpdateDateColumn,
  ManyToOne,
  JoinColumn,
  Unique,
} from "typeorm";
import { Project } from "./Project";

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

/**
 * Tmux pane 레이아웃 설정 엔티티.
 * projectId가 null이고 isGlobal이 true이면 글로벌 기본값,
 * projectId가 설정되면 해당 프로젝트의 오버라이드 설정.
 */
@Entity("pane_layout_configs")
@Unique(["projectId"])
export class PaneLayoutConfig {
  @PrimaryGeneratedColumn("uuid")
  id!: string;

  @Column({
    name: "layout_type",
    type: "varchar",
    length: 50,
  })
  layoutType!: PaneLayoutType;

  @Column({ type: "jsonb" })
  panes!: PaneCommand[];

  @ManyToOne(() => Project, { nullable: true, onDelete: "CASCADE" })
  @JoinColumn({ name: "project_id" })
  project!: Project | null;

  @Column({ name: "project_id", type: "uuid", nullable: true })
  projectId!: string | null;

  @Column({ name: "is_global", type: "boolean", default: false })
  isGlobal!: boolean;

  @CreateDateColumn({ name: "created_at" })
  createdAt!: Date;

  @UpdateDateColumn({ name: "updated_at" })
  updatedAt!: Date;
}
