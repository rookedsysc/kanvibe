import { getGitHubCliStatus as readGitHubCliStatus, installGitHubCli as runGitHubCliInstall } from "@/lib/githubCliDependency";

export async function getGitHubCliStatus(sshHost?: string | null) {
  return readGitHubCliStatus(sshHost);
}

export async function installGitHubCli(sshHost?: string | null) {
  await runGitHubCliInstall(sshHost);
  return readGitHubCliStatus(sshHost);
}
