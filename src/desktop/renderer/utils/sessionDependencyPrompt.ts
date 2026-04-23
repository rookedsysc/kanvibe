import type { SessionType } from "@/entities/KanbanTask";
import { getSessionDependencyStatus, installSessionDependency } from "@/desktop/renderer/actions/sessionDependency";

interface TranslationFn {
  (key: string, values?: Record<string, string | number | Date>): string;
}

export async function ensureSessionDependencyWithPrompt(
  sessionType: SessionType,
  sshHost: string | null | undefined,
  tCommon: TranslationFn,
): Promise<boolean> {
  const status = await getSessionDependencyStatus(sessionType, sshHost ?? null);
  if (status.available) {
    return true;
  }

  const target = status.isRemote
    ? tCommon("sessionDependency.remoteTarget", { host: status.sshHost ?? "remote" })
    : tCommon("sessionDependency.localTarget");
  const shouldInstall = window.confirm(
    tCommon("sessionDependency.installPrompt", {
      tool: status.toolName,
      target,
    }),
  );

  if (!shouldInstall) {
    return false;
  }

  const installed = await installSessionDependency(sessionType, sshHost ?? null);
  if (!installed.available) {
    throw new Error(
      tCommon("sessionDependency.installFailed", {
        tool: status.toolName,
        error: installed.blockedReason ?? "unknown error",
      }),
    );
  }

  return true;
}
