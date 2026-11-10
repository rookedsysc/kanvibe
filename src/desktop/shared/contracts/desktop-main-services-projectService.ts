

export interface ScanResult {
  registered: string[];
  skipped: string[];
  errors: string[];
  worktreeTasks: string[];
  registeredWorktrees: RegisteredWorktreeSummary[];
  hooksSetup: string[];
}

export interface RegisteredWorktreeSummary {
  taskId: string;
  projectName: string;
  branchName: string;
  worktreePath: string;
  sshHost: string | null;
}

export interface RegisteredProjectWorktreeSyncResult {
  worktreeTasks: string[];
  registeredWorktrees: RegisteredWorktreeSummary[];
  hooksSetup: string[];
  errors: string[];
  changed: boolean;
}
