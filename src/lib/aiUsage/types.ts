export type AiUsageProvider = "claude" | "codex" | "gemini";

/**
 * 사용량 창이 재는 기간. session은 5시간 창, weekly는 7일 창, monthly는 Codex 무료 등급의 30일 창이다.
 * model은 Gemini 모델별 쿼터처럼 묶일 기간 한도가 아예 없는 창에만 쓴다 —
 * 기간이 있는 모델 한도는 그 기간을 kind로 쓰고 modelName으로 갈라야 화면에서 같은 묶음에 선다.
 */
export type AiUsageWindowKind = "session" | "weekly" | "monthly" | "model";

export type AiUsageStatus = "ok" | "unavailable" | "error";

/**
 * 조회 실패 사유. 렌더러가 그대로 i18n 키로 쓰므로 사람이 읽는 문장이 아니라 안정적인 식별자를 담는다.
 * unavailable은 사용자가 아직 로그인하지 않았거나 CLI가 없는 정상적인 부재이고,
 * error는 호출이 실패했거나 응답을 해석할 수 없는 비정상 상태다.
 */
export type AiUsageFailureReason =
  | "missing-credentials"
  | "expired-credentials"
  | "keychain-unreadable"
  | "gemini-cli-not-found"
  | "rate-limited"
  | "empty-response"
  | "fetch-failed";

/** 조회 대상 계정 하나. configDir는 조회에만 쓰고 캐시에는 저장하지 않는다 */
export interface AiUsageAccount {
  provider: AiUsageProvider;
  accountId: string;
  label: string;
  configDir: string;
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
