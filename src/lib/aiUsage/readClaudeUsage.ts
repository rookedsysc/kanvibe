import path from "path";
import {
  AI_USAGE_REQUEST_TIMEOUT_MS,
  classifyUsageHttpFailure,
  createErrorUsage,
  createUnavailableUsage,
  createUsageResult,
  createUsageWindow,
} from "@/lib/aiUsage/shared";
import {
  hasUsableClaudeRefreshToken,
  isClaudeAccessTokenExpired,
  readClaudeAccessToken,
  readClaudeKeychainCredentials,
  readClaudeSubscriptionType,
  readClaudeTokenExpiresAt,
} from "@/lib/aiUsage/claudeCredentials";
import type {
  AiUsageAccount,
  AiUsageAccountResult,
  AiUsageWindow,
  AiUsageWindowKind,
} from "@/lib/aiUsage/types";
import { readTextFile } from "@/lib/hostFileAccess";

const CLAUDE_USAGE_URL = "https://api.anthropic.com/api/oauth/usage";

/**
 * Claude Code CLI가 이 엔드포인트에 붙이는 헤더를 그대로 맞춘다.
 * 공개 API가 아니라 CLI 전용 계약이라 값이 다르면 서버가 응답 형식을 바꾸거나 거절한다.
 */
const CLAUDE_OAUTH_BETA_HEADER = "oauth-2025-04-20";
const CLAUDE_CODE_USER_AGENT = "claude-code/2.1.0";

const TOO_MANY_REQUESTS_STATUS = 429;

/** `retry-after`가 비어 있을 때 쓸 간격. 실측 응답이 290초를 줬다 */
const CLAUDE_RATE_LIMIT_FALLBACK_SECONDS = 300;

/**
 * 429가 알려준 재시도 가능 시각.
 *
 * 이 엔드포인트는 5분에 대여섯 번이면 잠기고, 잠긴 뒤 더 부르면 남은 예산만 태운다.
 * 계정을 나눠도 한도는 같이 걸리므로 계정별이 아니라 모듈 하나가 기억한다.
 */
let usageRetryAllowedAtMs = 0;

interface ClaudeUsageWindowResponse {
  utilization?: unknown;
  used_percentage?: unknown;
  resets_at?: unknown;
}

/** 새 응답이 창을 담는 자리. 모델별 주간 한도는 여기에만 들어온다 */
interface ClaudeUsageLimitResponse {
  kind?: unknown;
  percent?: unknown;
  resets_at?: unknown;
  scope?: { model?: { display_name?: unknown } | null } | null;
}

interface ClaudeUsageResponse {
  five_hour?: ClaudeUsageWindowResponse;
  seven_day?: ClaudeUsageWindowResponse;
  limits?: unknown;
}

function toCredentialsPath(configDir: string): string {
  return path.join(configDir, ".credentials.json");
}

/**
 * 두 저장소 중 더 나중까지 쓸 수 있는 쪽을 가른다.
 *
 * macOS의 Claude Code는 로그인 결과를 Keychain에 쓰므로, 파일이 있다고 먼저 믿으면
 * config dir에 남은 묵은 `.credentials.json` 하나 때문에 재로그인해도 죽은 토큰을 계속 보내게 된다.
 * 만료 시각을 모르는 자격증명은 비교에서 지고, 양쪽 다 모르면 읽기가 싼 파일을 쓴다.
 */
function isKeychainFresher(fileCredentials: string, keychainCredentials: string): boolean {
  if (!keychainCredentials) {
    return false;
  }

  if (!fileCredentials) {
    return true;
  }

  const keychainExpiresAt = readClaudeTokenExpiresAt(keychainCredentials);
  const fileExpiresAt = readClaudeTokenExpiresAt(fileCredentials);
  return keychainExpiresAt !== null && (fileExpiresAt === null || keychainExpiresAt > fileExpiresAt);
}

/**
 * 파일과 Keychain 중 신선한 쪽을 읽기만 한다.
 *
 * refresh 토큰은 한 번 쓰면 회전돼서, KanVibe가 대신 갱신하면 Claude Code가 들고 있던 값이 무효가 되고
 * 두 곳에 서로 다른 로그인이 남는다. 그래서 갱신은 Claude Code만 하고 여기서는 저장소에 쓰지 않는다.
 */
async function readFreshClaudeCredentials(
  configDir: string,
): Promise<{ credentials: string; isKeychainUnreadable: boolean }> {
  const fileCredentials = await readTextFile(toCredentialsPath(configDir));
  const keychainResult = await readClaudeKeychainCredentials([configDir]);
  const keychainCredentials = keychainResult.outcome === "found" ? keychainResult.credentials : "";
  const isKeychainUnreadable = keychainResult.outcome === "unreadable";

  return {
    credentials: isKeychainFresher(fileCredentials, keychainCredentials)
      ? keychainCredentials
      : fileCredentials,
    isKeychainUnreadable,
  };
}

/**
 * 액세스 토큰을 쓸 수 없을 때, Claude Code가 풀 수 있는 부재와 사용자가 다시 로그인해야 하는 부재를 가른다.
 * 앞쪽은 로그인이 살아 있으므로 재로그인을 권하지 않는다.
 */
function createUnusableTokenUsage(account: AiUsageAccount, credentials: string): AiUsageAccountResult {
  return createUnavailableUsage(
    account,
    hasUsableClaudeRefreshToken(credentials) ? "awaiting-cli-refresh" : "expired-credentials",
  );
}

function toClaudeUsageWindow(
  raw: ClaudeUsageWindowResponse | undefined,
  kind: AiUsageWindowKind,
): AiUsageWindow | null {
  if (!raw) {
    return null;
  }

  const usedPercent = typeof raw.utilization === "number" ? raw.utilization : raw.used_percentage;
  return createUsageWindow(kind, usedPercent, raw.resets_at);
}

/**
 * 응답의 창 이름을 화면이 아는 종류로 옮긴다. 여기 없는 이름은 그릴 라벨이 없어 버린다.
 *
 * `weekly_scoped`는 7일 한도 안에서 모델 몫만 따로 센 창이라 `weekly_all`과 같은 7일로 묶는다.
 * 별도 종류로 두면 화면에서 5시간·7일과 나란히 서서 어느 기간의 한도인지 드러나지 않는다.
 */
const CLAUDE_LIMIT_KINDS: Record<string, { kind: AiUsageWindowKind; isModelScoped: boolean }> = {
  session: { kind: "session", isModelScoped: false },
  weekly_all: { kind: "weekly", isModelScoped: false },
  weekly_scoped: { kind: "weekly", isModelScoped: true },
};

function toClaudeLimitWindow(limit: ClaudeUsageLimitResponse): AiUsageWindow | null {
  const limitKind = typeof limit.kind === "string" ? CLAUDE_LIMIT_KINDS[limit.kind] : undefined;
  if (!limitKind) {
    return null;
  }

  if (!limitKind.isModelScoped) {
    return createUsageWindow(limitKind.kind, limit.percent, limit.resets_at);
  }

  // 모델 창은 모델 이름이 곧 라벨이라, 이름이 없으면 무엇의 한도인지 알릴 방법이 없다
  const modelName = limit.scope?.model?.display_name;
  return typeof modelName === "string" && modelName.trim()
    ? createUsageWindow(limitKind.kind, limit.percent, limit.resets_at, modelName.trim())
    : null;
}

/**
 * 새 응답은 `limits` 배열에 창을 담고, 모델별 주간 한도는 그쪽에만 들어온다.
 * 배열이 없는 옛 응답도 여전히 오므로 상위 필드를 폴백으로 남긴다.
 */
function toClaudeUsageWindows(payload: ClaudeUsageResponse): AiUsageWindow[] {
  const limits = Array.isArray(payload.limits) ? (payload.limits as ClaudeUsageLimitResponse[]) : [];
  const limitWindows = limits
    .map(toClaudeLimitWindow)
    .filter((window): window is AiUsageWindow => window !== null);
  if (limitWindows.length > 0) {
    return limitWindows;
  }

  return [
    toClaudeUsageWindow(payload.five_hour, "session"),
    toClaudeUsageWindow(payload.seven_day, "weekly"),
  ].filter((window): window is AiUsageWindow => window !== null);
}

function toRetryAllowedAtMs(response: Response): number {
  const retryAfterSeconds = Number(response.headers.get("retry-after"));
  const backoffSeconds = Number.isFinite(retryAfterSeconds) && retryAfterSeconds > 0
    ? retryAfterSeconds
    : CLAUDE_RATE_LIMIT_FALLBACK_SECONDS;
  return Date.now() + backoffSeconds * 1000;
}

export async function readClaudeUsage(account: AiUsageAccount): Promise<AiUsageAccountResult> {
  const { credentials, isKeychainUnreadable } = await readFreshClaudeCredentials(account.configDir);
  const accessToken = readClaudeAccessToken(credentials);
  if (!accessToken) {
    return createUnavailableUsage(
      account,
      isKeychainUnreadable ? "keychain-unreadable" : "missing-credentials",
    );
  }

  // 만료된 토큰은 보내도 거절될 뿐이라, 몇 번 안 되는 호출 예산을 거기에 쓰지 않는다
  if (isClaudeAccessTokenExpired(credentials)) {
    return createUnusableTokenUsage(account, credentials);
  }

  if (Date.now() < usageRetryAllowedAtMs) {
    return createErrorUsage(account, "rate-limited");
  }

  let response: Response;
  try {
    response = await fetch(CLAUDE_USAGE_URL, {
      headers: {
        Authorization: `Bearer ${accessToken}`,
        "anthropic-beta": CLAUDE_OAUTH_BETA_HEADER,
        "User-Agent": CLAUDE_CODE_USER_AGENT,
      },
      signal: AbortSignal.timeout(AI_USAGE_REQUEST_TIMEOUT_MS),
    });
  } catch {
    return createErrorUsage(account, "fetch-failed");
  }

  if (!response.ok) {
    if (response.status === TOO_MANY_REQUESTS_STATUS) {
      usageRetryAllowedAtMs = toRetryAllowedAtMs(response);
    }

    const failure = classifyUsageHttpFailure(account, response.status);
    return failure.reason === "expired-credentials"
      ? createUnusableTokenUsage(account, credentials)
      : failure;
  }

  let payload: ClaudeUsageResponse;
  try {
    payload = (await response.json()) as ClaudeUsageResponse;
  } catch {
    return createErrorUsage(account, "fetch-failed");
  }

  const windows = toClaudeUsageWindows(payload);
  if (windows.length === 0) {
    return createErrorUsage(account, "empty-response");
  }

  return createUsageResult(account, windows, readClaudeSubscriptionType(credentials));
}
