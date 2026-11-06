

export interface GitHubCliStatus {
  toolName: "gh";
  sshHost: string | null;
  isRemote: boolean;
  available: boolean;
  blockedReason: string | null;
}
