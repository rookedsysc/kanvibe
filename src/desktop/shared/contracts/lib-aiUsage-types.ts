

export type AiUsageProvider = "claude" | "codex" | "gemini";

export type AiUsageWindowKind = "session" | "weekly" | "monthly" | "model";

export type AiUsageStatus = "ok" | "unavailable" | "error";

export type AiUsageFailureReason =
  | "missing-credentials"
  | "expired-credentials"
  | "keychain-unreadable"
  | "gemini-cli-not-found"
  | "rate-limited"
  | "empty-response"
  | "fetch-failed";

export interface AiUsageAccount {
  provider: AiUsageProvider;
  accountId: string;
  label: string;
  /** 자격증명이 놓인 디렉터리 */
  configDir: string;
  /** CLI에 계정 위치를 알릴 때 넘기는 루트. provider에 따라 configDir와 같을 수도 있다 */
  accountRoot: string;
}

export interface AiUsageWindow {
  kind: AiUsageWindowKind;
  /** kind가 "model"일 때만 채워지는 표시용 모델 이름 */
  modelName: string | null;
  usedPercent: number;
  resetsAt: string | null;
}

export interface AiUsageAccountResult {
  provider: AiUsageProvider;
  /** 발견 시점의 계정 식별자. 캐시에 넣을 때는 해시해서 전체 값을 남기지 않는다 */
  accountId: string;
  label: string;
  status: AiUsageStatus;
  /** 구독 등급 표시값. Codex의 plan_type처럼 응답이 알려줄 때만 채운다 */
  planName: string | null;
  windows: AiUsageWindow[];
  reason: AiUsageFailureReason | null;
  fetchedAt: string;
}

export interface AiUsageSnapshot {
  accounts: AiUsageAccountResult[];
  fetchedAt: string;
}
