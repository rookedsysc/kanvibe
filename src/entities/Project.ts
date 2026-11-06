

export interface Project {
id: string;
name: string;
repoPath: string;
defaultBranch: string;
sshHost: string | null;
isWorktree: boolean;
color: string | null;
iconDataUrl: string | null;
createdAt: Date;
}