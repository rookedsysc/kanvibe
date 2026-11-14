export enum HerdrSessionMode {
  ATTACH_EXISTING = "attachExisting",
  REBUILD_STOPPED = "rebuildStopped",
}

export enum HerdrOperationPhase {
  APPLYING = "applying",
  VERIFYING = "verifying",
  COMMITTED = "committed",
  COMPLETED = "completed",
  FAILED = "failed",
  ATTENTION = "attention",
  CANCELLED = "cancelled",
  REVERTED = "reverted",
}

export interface HerdrSessionError {
  code: string;
  message: string;
  retryable: boolean;
  operationId: string | null;
}

export type HerdrSessionResult<T> =
  | { ok: true; value: T }
  | { ok: false; error: HerdrSessionError };

export interface HerdrTaskBinding {
  sshHost: string | null;
  sessionType: "herdr";
  sessionName: string;
  worktreePath: string | null;
}

export interface HerdrSessionWorkspace {
  workspaceId: string;
  label: string;
  cwd: string;
  tabCount: number;
}

export interface HerdrScannedSession {
  name: string;
  running: boolean;
  sessionDir: string | null;
  snapshotState: "valid" | "missing" | "unsupported" | "unreadable";
  workspaces: HerdrSessionWorkspace[];
  taskIds: string[];
  error: string | null;
}

export interface HerdrSessionScan {
  scanId: string;
  sshHost: string | null;
  observedAt: string;
  fingerprint: string;
  sessions: HerdrScannedSession[];
  tasks: { id: string; title: string; sessionName: string; worktreePath: string | null }[];
}

export interface HerdrSessionPlan {
  planId: string;
  fingerprint: string;
  taskId: string;
  mode: HerdrSessionMode;
  source: { sshHost: string | null; sessionName: string; workspaceId: string };
  targetSession: string;
  eligibility: "ready" | "blocked";
  previousBinding: HerdrTaskBinding;
  importTask?: { title: string };
  preserved: string[];
  recreated: string[];
  unsupported: string[];
  blockers: string[];
}

export interface HerdrSessionOperation {
  operationId: string;
  plan: HerdrSessionPlan;
  phase: HerdrOperationPhase;
  targetWorkspaceId: string | null;
  backupLocation: string | null;
  bindingCommitted?: boolean;
  error: HerdrSessionError | null;
  createdAt: string;
  updatedAt: string;
}

export interface HerdrCreatePlanRequest {
  scanId: string;
  taskId: string;
  sourceSession: string;
  sourceWorkspaceId: string;
  mode: HerdrSessionMode;
}

export interface HerdrExecutePlanRequest {
  planId: string;
  expectedFingerprint: string;
  operationId: string;
}
