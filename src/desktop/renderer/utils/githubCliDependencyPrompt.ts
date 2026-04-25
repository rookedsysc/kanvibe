import { getGitHubCliStatus, installGitHubCli } from "@/desktop/renderer/actions/githubCliDependency";

interface TranslationFn {
  (key: string, values?: Record<string, string | number | Date>): string;
}

export async function ensureGitHubCliWithPrompt(
  sshHost: string | null | undefined,
  tCommon: TranslationFn,
): Promise<boolean> {
  const status = await getGitHubCliStatus(sshHost ?? null);
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

  const installed = await installGitHubCli(sshHost ?? null);
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
