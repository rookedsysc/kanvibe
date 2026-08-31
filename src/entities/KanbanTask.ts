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
import { TaskPriority } from "./TaskPriority";

export enum TaskStatus {
  TODO = "todo",
  PROGRESS = "progress",
  PENDING = "pending",
  REVIEW = "review",
  DONE = "done",
}

/** 보드 컬럼과 동일한 상태 진행 순서. 상태를 순서대로 나열해야 하는 곳(vim 이동 명령, 커맨드 팔레트 등)이 공유한다 */
export const TASK_STATUS_ORDER: readonly TaskStatus[] = [
  TaskStatus.TODO,
  TaskStatus.PROGRESS,
  TaskStatus.PENDING,
  TaskStatus.REVIEW,
  TaskStatus.DONE,
];

export enum SessionType {
  TMUX = "tmux",
  ZELLIJ = "zellij",
  /** 멀티플렉서 없이 KanVibe가 PTY를 직접 소유하는 세션. 탭도 KanVibe가 관리한다 */
  TERMINAL = "terminal",
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

  @Column({ type: "simple-enum", enum: TaskStatus, default: TaskStatus.TODO })
  status!: TaskStatus;

  @Column({ name: "branch_name", type: "varchar", length: 255, nullable: true })
  branchName!: string | null;

  @Column({ name: "worktree_path", type: "varchar", length: 500, nullable: true })
  worktreePath!: string | null;

  @Column({ name: "session_type", type: "simple-enum", enum: SessionType, nullable: true })
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

  @Column({ name: "project_id", type: "varchar", length: 36, nullable: true })
  projectId!: string | null;

  @Column({ name: "base_branch", type: "varchar", length: 255, nullable: true })
  baseBranch!: string | null;

  @Column({ name: "pr_url", type: "varchar", length: 500, nullable: true })
  prUrl!: string | null;

  @Column({ type: "simple-enum", enum: TaskPriority, nullable: true, default: null })
  priority!: TaskPriority | null;

  @CreateDateColumn({ name: "created_at" })
  createdAt!: Date;

  @UpdateDateColumn({ name: "updated_at" })
  updatedAt!: Date;
}
