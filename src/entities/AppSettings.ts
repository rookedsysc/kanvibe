import "reflect-metadata";
import {
  Entity,
  PrimaryGeneratedColumn,
  Column,
  CreateDateColumn,
  UpdateDateColumn,
} from "typeorm";

/**
 * 앱 전역 설정을 저장하는 키-값 엔티티.
 * 사이드바 기본 접힘 상태 등 UI 설정을 관리한다.
 */
@Entity("app_settings")
export class AppSettings {
  @PrimaryGeneratedColumn("uuid")
  id!: string;

  @Column({ type: "varchar", length: 100, unique: true })
  key!: string;

  @Column({ type: "text" })
  value!: string;

  @CreateDateColumn({ name: "created_at" })
  createdAt!: Date;

  @UpdateDateColumn({ name: "updated_at" })
  updatedAt!: Date;
}
