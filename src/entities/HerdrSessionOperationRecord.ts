import "reflect-metadata";
import { Column, Entity, PrimaryColumn } from "typeorm";

/** 태스크를 삭제해도 미완료 원격 작업의 복구 자료는 남겨야 하므로 FK로 함께 지우지 않는다. */
@Entity("herdr_session_operations")
export class HerdrSessionOperationRecord {
  @PrimaryColumn({ name: "operation_id", type: "varchar", length: 36 })
  operationId!: string;

  @Column({ name: "task_id", type: "varchar", length: 36 })
  taskId!: string;

  /** 미지 상태의 원문을 이전 버전에서도 보존하도록 DB enum 제약은 두지 않는다. */
  @Column({ type: "varchar", length: 32 })
  phase!: string;

  @Column({ type: "text" })
  payload!: string;

  @Column({ name: "created_at", type: "varchar", length: 30 })
  createdAt!: string;

  @Column({ name: "updated_at", type: "varchar", length: 30 })
  updatedAt!: string;
}
