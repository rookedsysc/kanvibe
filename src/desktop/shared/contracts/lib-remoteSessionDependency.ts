import type { SessionType } from "@/entities/KanbanTask";

export interface SessionDependencyStatus {
  sessionType: SessionType;
  /** 설치가 필요 없는 세션 타입은 null이다 */
  toolName: "tmux" | "zellij" | null;
  sshHost: string | null;
  isRemote: boolean;
  available: boolean;
  blockedReason: string | null;
}
