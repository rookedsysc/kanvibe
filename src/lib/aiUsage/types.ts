export type AiUsageProvider = "claude" | "codex" | "gemini";

/**
 * 사용량 창의 종류. provider마다 창 구조가 다르지만 렌더러는 이 값만 보고 라벨을 고른다.
 * session은 5시간 창, weekly는 7일 창, model은 Gemini처럼 모델별로 쪼개진 창이다.
 */
export type AiUsageWindowKind = "session" | "weekly" | "model";

export type AiUsageStatus = "ok" | "unavailable" | "error";

/**
 * 조회 실패 사유. 렌더러가 그대로 i18n 키로 쓰므로 사람이 읽는 문장이 아니라 안정적인 식별자를 담는다.
 * unavailable은 사용자가 아직 로그인하지 않았거나 CLI가 없는 정상적인 부재이고,
 * error는 호출이 실패했거나 응답을 해석할 수 없는 비정상 상태다.
 */
export type AiUsageFailureReason =
  | "missing-credentials"
  | "expired-credentials"
  | "gemini-cli-not-found"
  | "rate-limited"
  | "empty-response"
  | "fetch-failed";

export interface AiUsageWindow {
  kind: AiUsageWindowKind;
  /** kind가 "model"일 때만 채워지는 표시용 모델 이름 */
  modelName: string | null;
  usedPercent: number;
  resetsAt: string | null;
}

export interface AiUsageProviderResult {
  provider: AiUsageProvider;
  status: AiUsageStatus;
  /** 구독 등급 표시값. Codex의 plan_type처럼 응답이 알려줄 때만 채운다 */
  planName: string | null;
  windows: AiUsageWindow[];
  reason: AiUsageFailureReason | null;
  fetchedAt: string;
}

export interface AiUsageSnapshot {
  providers: AiUsageProviderResult[];
  fetchedAt: string;
}
