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
import {
  readClaudeAccessToken,
  readClaudeKeychainCredentials,
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

interface ClaudeUsageWindowResponse {
  utilization?: unknown;
  used_percentage?: unknown;
  resets_at?: unknown;
}

interface ClaudeUsageResponse {
  five_hour?: ClaudeUsageWindowResponse;
  seven_day?: ClaudeUsageWindowResponse;
}

/**
 * 액세스 토큰이 만료 임박이면 갱신한 뒤 조회한다.
 *
 * 자격증명 파일이 없으면 macOS Keychain이 원본이다. Keychain은 Claude Code의 소유이고
 * 회전된 refresh 토큰을 그쪽에 되쓸 안전한 방법이 없어, 그 경우에는 갱신하지 않고 있는 값만 쓴다.
 * 갱신했는데 되쓰지 못하면 사용자의 CLI 로그인까지 끊기므로 갱신과 되쓰기는 함께 가능할 때만 한다.
 *
 * refresh 토큰은 한 번 쓰면 회전되므로, 갱신에 성공하면 조회보다 먼저 파일에 되쓴다.
 * 되쓰기가 실패하면 저장된 refresh 토큰은 이미 서버에서 무효가 된 상태라 CLI 재로그인이 필요해진다 —
 * 복구할 방법이 없으므로 최소한 눈에 띄게 남긴다.
 */
async function readFreshClaudeCredentials(
  configDir: string,
): Promise<{ credentials: string; isKeychainUnreadable: boolean }> {
  const credentialsPath = path.join(configDir, ".credentials.json");
  const storedCredentials = await readTextFile(credentialsPath);
  if (!storedCredentials) {
    const keychainResult = await readClaudeKeychainCredentials();
    return {
      credentials: keychainResult.outcome === "found" ? keychainResult.credentials : "",
      isKeychainUnreadable: keychainResult.outcome === "unreadable",
    };
  }

  if (!isClaudeTokenExpiring(storedCredentials)) {
    return { credentials: storedCredentials, isKeychainUnreadable: false };
  }

  const refreshedCredentials = await refreshClaudeCredentials(storedCredentials);
  if (!refreshedCredentials) {
    return { credentials: storedCredentials, isKeychainUnreadable: false };
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

export async function readClaudeUsage(account: AiUsageAccount): Promise<AiUsageAccountResult> {
  const { credentials, isKeychainUnreadable } = await readFreshClaudeCredentials(account.configDir);
  const accessToken = readClaudeAccessToken(credentials);
  if (!accessToken) {
    return createUnavailableUsage(
      account,
      isKeychainUnreadable ? "keychain-unreadable" : "missing-credentials",
    );
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
    return classifyUsageHttpFailure(account, response.status);
  }

  let payload: ClaudeUsageResponse;
  try {
    payload = (await response.json()) as ClaudeUsageResponse;
  } catch {
    return createErrorUsage(account, "fetch-failed");
  }

  const windows = [
    toClaudeUsageWindow(payload.five_hour, "session"),
    toClaudeUsageWindow(payload.seven_day, "weekly"),
  ].filter((window): window is AiUsageWindow => window !== null);

  if (windows.length === 0) {
    return createErrorUsage(account, "empty-response");
  }

  return createUsageResult(account, windows);
}
