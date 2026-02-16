import "reflect-metadata";
import {
  Entity,
  PrimaryGeneratedColumn,
  Column,
  CreateDateColumn,
  UpdateDateColumn,
  ManyToOne,
  JoinColumn,
} from "typeorm";
import { Project } from "./Project";

export enum TaskStatus {
  TODO = "todo",
  PROGRESS = "progress",
  PENDING = "pending",
  REVIEW = "review",
  DONE = "done",
}

export enum SessionType {
  TMUX = "tmux",
  ZELLIJ = "zellij",
}

/**
 * Kanban 보드의 작업 항목을 나타내는 엔티티.
 * 각 작업은 git worktree, 터미널 세션, SSH 연결 정보를 포함할 수 있다.
 */
@Entity("kanban_tasks")
export class KanbanTask {
  @PrimaryGeneratedColumn("uuid")
  id!: string;

  @Column({ type: "varchar", length: 255 })
  title!: string;

  @Column({ type: "text", nullable: true })
  description!: string | null;

  @Column({ type: "enum", enum: TaskStatus, default: TaskStatus.TODO })
  status!: TaskStatus;

  @Column({ name: "branch_name", type: "varchar", length: 255, nullable: true, unique: true })
  branchName!: string | null;

  @Column({ name: "worktree_path", type: "varchar", length: 500, nullable: true })
  worktreePath!: string | null;

  @Column({ name: "session_type", type: "enum", enum: SessionType, nullable: true })
  sessionType!: SessionType | null;

  @Column({ name: "session_name", type: "varchar", length: 255, nullable: true })
  sessionName!: string | null;

  @Column({ name: "ssh_host", type: "varchar", length: 255, nullable: true })
  sshHost!: string | null;

  @Column({ name: "agent_type", type: "varchar", length: 50, nullable: true })
  agentType!: string | null;

  @ManyToOne(() => Project, { nullable: true, onDelete: "SET NULL" })
  @JoinColumn({ name: "project_id" })
  project!: Project | null;

  @Column({ name: "project_id", type: "uuid", nullable: true })
  projectId!: string | null;

  @Column({ name: "base_branch", type: "varchar", length: 255, nullable: true })
  baseBranch!: string | null;

  @Column({ name: "pr_url", type: "varchar", length: 500, nullable: true })
  prUrl!: string | null;

  @Column({ name: "display_order", type: "int", default: 0 })
  displayOrder!: number;

  @CreateDateColumn({ name: "created_at" })
  createdAt!: Date;

  @UpdateDateColumn({ name: "updated_at" })
  updatedAt!: Date;
}
