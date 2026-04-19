import { SessionType } from "@/entities/KanbanTask";
import { getSessionDependencyStatus as readSessionDependencyStatus, installSessionDependency as runSessionDependencyInstall } from "@/lib/remoteSessionDependency";

export async function getSessionDependencyStatus(
  sessionType: SessionType,
  sshHost?: string | null,
) {
  return readSessionDependencyStatus(sessionType, sshHost);
}

export async function installSessionDependency(
  sessionType: SessionType,
  sshHost?: string | null,
) {
  await runSessionDependencyInstall(sessionType, sshHost);
  return readSessionDependencyStatus(sessionType, sshHost);
}
