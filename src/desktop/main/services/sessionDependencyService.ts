import { SessionType } from "@/entities/KanbanTask";
import { replaceHerdrRemoteServer } from "@/lib/terminal";
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

/** 원격 Herdr 서버 교체 확인은 앱이 실행되는 동안 서버마다 한 번만 묻는다. 창이 여러 개여도 처음 요청한 창 하나만 묻는다 */
const claimedHerdrServerHandoffPrompts = new Set<string>();

export function claimHerdrServerHandoffPrompt(sshHost: string, herdrSession: string): boolean {
  const promptKey = JSON.stringify([sshHost, herdrSession]);
  if (claimedHerdrServerHandoffPrompts.has(promptKey)) return false;
  claimedHerdrServerHandoffPrompts.add(promptKey);
  return true;
}

export async function replaceHerdrServer(sshHost: string, herdrSession: string): Promise<void> {
  await replaceHerdrRemoteServer(sshHost, herdrSession);
}
