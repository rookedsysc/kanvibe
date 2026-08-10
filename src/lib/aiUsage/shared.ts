import type {
  AiUsageAccount,
  AiUsageAccountResult,
  AiUsageFailureReason,
  AiUsageWindow,
  AiUsageWindowKind,
} from "@/lib/aiUsage/types";

/** provider 응답이 느릴 때 패널 전체가 멈추지 않도록 각 호출에 거는 상한 */
export const AI_USAGE_REQUEST_TIMEOUT_MS = 10_000;

/**
 * 초 단위 epoch와 밀리초 단위 epoch를 가르는 경계.
 * 1e10초는 서기 2286년이고 1e10밀리초는 1970년이므로, 이보다 크면 밀리초로 본다.
 */
const EPOCH_SECONDS_UPPER_BOUND = 10_000_000_000;

function clampUsedPercent(value: unknown): number | null {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return null;
  }

  return Math.min(100, Math.max(0, value));
}

/**
 * provider마다 reset 시각을 ISO 문자열, epoch 초, epoch 밀리초 중 아무 형태로나 준다.
 * 어느 쪽이든 ISO 문자열 하나로 눌러서 렌더러가 형식을 몰라도 되게 한다.
 */
function toIsoResetTime(value: unknown): string | null {
  if (typeof value === "number" && Number.isFinite(value)) {
    const milliseconds = value > EPOCH_SECONDS_UPPER_BOUND ? value : value * 1000;
    return toIsoStringOrNull(milliseconds);
  }

  if (typeof value !== "string" || !value.trim()) {
    return null;
  }

  const numericValue = Number(value);
  if (Number.isFinite(numericValue)) {
    const milliseconds = numericValue > EPOCH_SECONDS_UPPER_BOUND ? numericValue : numericValue * 1000;
    return toIsoStringOrNull(milliseconds);
  }

  return toIsoStringOrNull(Date.parse(value));
}

function toIsoStringOrNull(milliseconds: number): string | null {
  if (!Number.isFinite(milliseconds)) {
    return null;
  }

  try {
    return new Date(milliseconds).toISOString();
  } catch {
    return null;
  }
}

export function createUsageWindow(
  kind: AiUsageWindowKind,
  usedPercent: unknown,
  resetsAt: unknown,
  modelName: string | null = null,
): AiUsageWindow | null {
  const clampedPercent = clampUsedPercent(usedPercent);
  if (clampedPercent === null) {
    return null;
  }

  return {
    kind,
    modelName,
    usedPercent: clampedPercent,
    resetsAt: toIsoResetTime(resetsAt),
  };
}

export function createUsageResult(
  account: AiUsageAccount,
  windows: AiUsageWindow[],
  planName: string | null = null,
): AiUsageAccountResult {
  return {
    provider: account.provider,
    accountId: account.accountId,
    label: account.label,
    status: "ok",
    planName,
    windows,
    reason: null,
    fetchedAt: new Date().toISOString(),
  };
}

/** 아직 로그인하지 않았거나 CLI가 없는 정상적인 부재 */
export function createUnavailableUsage(
  account: AiUsageAccount,
  reason: AiUsageFailureReason,
): AiUsageAccountResult {
  return createFailedUsage(account, "unavailable", reason);
}

/** 호출이 실패했거나 응답을 해석할 수 없는 비정상 상태 */
export function createErrorUsage(
  account: AiUsageAccount,
  reason: AiUsageFailureReason,
): AiUsageAccountResult {
  return createFailedUsage(account, "error", reason);
}

function createFailedUsage(
  account: AiUsageAccount,
  status: "unavailable" | "error",
  reason: AiUsageFailureReason,
): AiUsageAccountResult {
  return {
    provider: account.provider,
    accountId: account.accountId,
    label: account.label,
    status,
    planName: null,
    windows: [],
    reason,
    fetchedAt: new Date().toISOString(),
  };
}

/**
 * 인증 실패는 재로그인으로 풀리는 부재이고 429는 잠시 뒤 재시도할 문제이므로 서로 다른 상태로 가른다.
 * 나머지 비 2xx는 스키마가 바뀌었을 수도 있으므로 앱을 죽이지 않고 error로만 떨어뜨린다.
 */
export function classifyUsageHttpFailure(
  account: AiUsageAccount,
  statusCode: number,
): AiUsageAccountResult {
  if (statusCode === 401 || statusCode === 403) {
    return createUnavailableUsage(account, "expired-credentials");
  }

  if (statusCode === 429) {
    return createErrorUsage(account, "rate-limited");
  }

  return createErrorUsage(account, "fetch-failed");
}
