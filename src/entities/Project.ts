import "reflect-metadata";
import {
  Entity,
  PrimaryGeneratedColumn,
  Column,
  CreateDateColumn,
} from "typeorm";

/**
 * 등록된 git 프로젝트를 나타내는 엔티티.
 * sshHost가 설정되면 원격 프로젝트로 간주하여 SSH를 통해 git 연산을 수행한다.
 */
@Entity("projects")
export class Project {
  @PrimaryGeneratedColumn("uuid")
  id!: string;

  @Column({ type: "varchar", length: 255, unique: true })
  name!: string;

  @Column({ name: "repo_path", type: "varchar", length: 500 })
  repoPath!: string;

  @Column({ name: "default_branch", type: "varchar", length: 255, default: "main" })
  defaultBranch!: string;

  @Column({ name: "ssh_host", type: "varchar", length: 255, nullable: true })
  sshHost!: string | null;

  @Column({ name: "is_worktree", type: "boolean", default: false })
  isWorktree!: boolean;

  @Column({ name: "color", type: "varchar", length: 7, nullable: true, default: null })
  color!: string | null;

  @CreateDateColumn({ name: "created_at" })
  createdAt!: Date;
}
