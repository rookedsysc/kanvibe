import { invokeDesktop } from "@/desktop/renderer/ipc";
import type { GitHubCliStatus } from "@/desktop/shared/contracts/lib-githubCliDependency";

export function getGitHubCliStatus(sshHost?: string | null): Promise<GitHubCliStatus> {
  return invokeDesktop("githubCliDependency", "getGitHubCliStatus", sshHost ?? null);
}

export function installGitHubCli(sshHost?: string | null): Promise<GitHubCliStatus> {
  return invokeDesktop("githubCliDependency", "installGitHubCli", sshHost ?? null);
}
