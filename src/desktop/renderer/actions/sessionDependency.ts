import type { SessionType } from "@/entities/KanbanTask";
import { invokeDesktop } from "@/desktop/renderer/ipc";
import type { SessionDependencyStatus } from "@/lib/remoteSessionDependency";

export function getSessionDependencyStatus(
  sessionType: SessionType,
  sshHost?: string | null,
): Promise<SessionDependencyStatus> {
  return invokeDesktop("sessionDependency", "getSessionDependencyStatus", sessionType, sshHost ?? null);
}

export function installSessionDependency(
  sessionType: SessionType,
  sshHost?: string | null,
): Promise<SessionDependencyStatus> {
  return invokeDesktop("sessionDependency", "installSessionDependency", sessionType, sshHost ?? null);
}

export function claimHerdrServerHandoffPrompt(sshHost: string, herdrSession: string): Promise<boolean> {
  return invokeDesktop("sessionDependency", "claimHerdrServerHandoffPrompt", sshHost, herdrSession);
}

export function replaceHerdrServer(sshHost: string, herdrSession: string): Promise<void> {
  return invokeDesktop("sessionDependency", "replaceHerdrServer", sshHost, herdrSession);
}
