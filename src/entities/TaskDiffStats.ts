import "reflect-metadata";
import { Entity, Column, PrimaryColumn, UpdateDateColumn, ManyToOne, JoinColumn } from "typeorm";
import { KanbanTask } from "./KanbanTask";

/**
 * 태스크의 마지막 변경 집계를 보관한다.
 *
 * 보드는 진행 중인 태스크만 git을 다시 돌리고 나머지 칸은 여기 저장된 값을 보여준다.
 * 태스크 행이 아니라 별도 테이블에 두는 이유는 두 가지다. 집계는 태스크가 바뀌지 않아도 갱신되는 파생값이고,
 * 태스크 행에 쓰면 `updatedAt`이 밀려 그 값으로 정렬하는 보드가 매 갱신마다 카드를 재배열한다.
 */
@Entity("task_diff_stats")
export class TaskDiffStatsRecord {
  @PrimaryColumn({ name: "task_id", type: "varchar", length: 36 })
  taskId!: string;

  @ManyToOne(() => KanbanTask, { onDelete: "CASCADE" })
  @JoinColumn({ name: "task_id" })
  task!: KanbanTask;

  @Column({ name: "file_count", type: "integer" })
  fileCount!: number;

  @Column({ type: "integer" })
  additions!: number;

  @Column({ type: "integer" })
  deletions!: number;

  @UpdateDateColumn({ name: "updated_at" })
  updatedAt!: Date;
}
