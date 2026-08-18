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
  isClaudeTokenExpiring,
  refreshClaudeCredentials,
} from "@/lib/aiUsage/claudeOAuthRefresh";
import { writeCredentialsAtomically } from "@/lib/aiUsage/atomicCredentialsWrite";
import { refreshCredentialsThroughCli } from "@/lib/aiUsage/providerCli";
import {
  readClaudeAccessToken,
  readClaudeKeychainCredentials,
  readClaudeSubscriptionType,
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

/**
 * Keychain에 있는 자격증명을 갱신한다.
 *
 * Keychain 항목은 Claude Code의 소유라 KanVibe가 회전된 refresh 토큰을 되쓰면 CLI 쪽 로그인이 깨진다.
 * 그래서 갱신 자체를 소유자에게 맡기고 결과만 다시 읽는다. 예전에는 여기서 포기하고 사용자에게
 * "터미널에서 claude를 한 번 실행하라"고 떠넘겼는데, 그 실행을 앱이 대신하는 것이 이 함수다.
 */
async function refreshKeychainCredentials(configDir: string): Promise<string> {
  await refreshCredentialsThroughCli("claude", configDir);

  const refreshedResult = await readClaudeKeychainCredentials([configDir]);
  return refreshedResult.outcome === "found" ? refreshedResult.credentials : "";
}

/**
 * 자격증명 파일이 없으면 macOS Keychain이 원본이다.
 * 만료 임박이 아니면 있는 값을 그대로 쓰고, 만료 임박일 때만 CLI에 갱신을 맡긴다.
 */
async function readFreshKeychainCredentials(
  configDir: string,
): Promise<{ credentials: string; isKeychainUnreadable: boolean }> {
  const keychainResult = await readClaudeKeychainCredentials([configDir]);
  if (keychainResult.outcome !== "found") {
    return { credentials: "", isKeychainUnreadable: keychainResult.outcome === "unreadable" };
  }

  if (!isClaudeTokenExpiring(keychainResult.credentials)) {
    return { credentials: keychainResult.credentials, isKeychainUnreadable: false };
  }

  const refreshedCredentials = await refreshKeychainCredentials(configDir);
  return {
    credentials: refreshedCredentials || keychainResult.credentials,
    isKeychainUnreadable: false,
  };
}

/**
 * 액세스 토큰이 만료 임박이면 갱신한 뒤 조회한다.
 *
 * refresh 토큰은 한 번 쓰면 회전되므로, 갱신에 성공하면 조회보다 먼저 파일에 되쓴다.
 * 되쓰기가 실패하면 저장된 refresh 토큰은 이미 서버에서 무효가 된 상태라 CLI 재로그인이 필요해진다 —
 * 복구할 방법이 없으므로 최소한 눈에 띄게 남긴다.
 *
 * 직접 갱신이 실패하면 CLI에 한 번 맡겨 본다. 저장된 refresh 토큰이 이미 회전된 뒤라면
 * 최신 값을 아는 쪽은 CLI뿐이고, 그 경우에도 사용자가 터미널로 나갈 이유는 없다.
 */
async function readFreshClaudeCredentials(
  configDir: string,
): Promise<{ credentials: string; isKeychainUnreadable: boolean }> {
  const credentialsPath = path.join(configDir, ".credentials.json");
  const storedCredentials = await readTextFile(credentialsPath);
  if (!storedCredentials) {
    return readFreshKeychainCredentials(configDir);
  }

  if (!isClaudeTokenExpiring(storedCredentials)) {
    return { credentials: storedCredentials, isKeychainUnreadable: false };
  }

  const refreshedCredentials = await refreshClaudeCredentials(storedCredentials);
  if (!refreshedCredentials) {
    await refreshCredentialsThroughCli("claude", configDir);
    const cliRefreshedCredentials = await readTextFile(credentialsPath);
    return {
      credentials: cliRefreshedCredentials || storedCredentials,
      isKeychainUnreadable: false,
    };
  }

  try {
    await writeCredentialsAtomically(credentialsPath, refreshedCredentials);
  } catch (error) {
    console.error(
      `[ai-usage] 갱신한 Claude 자격증명을 저장하지 못했습니다. ${credentialsPath}의 refresh 토큰이 무효일 수 있어 재로그인이 필요할 수 있습니다:`,
      error instanceof Error ? error.message : error,
    );
  }

  return { credentials: refreshedCredentials, isKeychainUnreadable: false };
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
    return classifyUsageHttpFailure(account, response.status);
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
