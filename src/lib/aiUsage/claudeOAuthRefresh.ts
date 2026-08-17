import { AI_USAGE_REQUEST_TIMEOUT_MS } from "@/lib/aiUsage/shared";

/**
 * 토큰 엔드포인트와 client id는 설치된 `claude` 바이너리(2.1.226)에서 확인한 공개값이다.
 * CLI와 같은 값을 써야 서버가 같은 계약으로 응답한다.
 */
const CLAUDE_OAUTH_TOKEN_URL = "https://platform.claude.com/v1/oauth/token";
const CLAUDE_OAUTH_CLIENT_ID = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

/** CLI가 쓰는 것과 같은 5분 여유. 조회 도중 토큰이 만료되는 일을 막는다 */
const TOKEN_EXPIRY_BUFFER_MS = 5 * 60 * 1000;

interface ClaudeOAuthBlock {
  accessToken?: unknown;
  refreshToken?: unknown;
  expiresAt?: unknown;
  scopes?: unknown;
  [key: string]: unknown;
}

interface ClaudeCredentials {
  claudeAiOauth?: ClaudeOAuthBlock;
  [key: string]: unknown;
}

export interface ClaudeTokenResponse {
  access_token?: unknown;
  expires_in?: unknown;
  refresh_token?: unknown;
  scope?: unknown;
}

function parseClaudeOAuthBlock(credentialsJson: string): ClaudeOAuthBlock | null {
  try {
    const parsed = JSON.parse(credentialsJson) as ClaudeCredentials;
    const oauth = parsed?.claudeAiOauth;
    return oauth && typeof oauth === "object" && !Array.isArray(oauth) ? oauth : null;
  } catch {
    return null;
  }
}

function readRefreshToken(credentialsJson: string): string | null {
  const refreshToken = parseClaudeOAuthBlock(credentialsJson)?.refreshToken;
  return typeof refreshToken === "string" && refreshToken.trim() ? refreshToken.trim() : null;
}

/**
 * 만료 시각을 알 수 없는 자격증명은 무기한 신뢰하지 않고 갱신 대상으로 본다.
 * OAuth 블록 자체가 없으면 갱신할 대상이 없으므로 false다.
 */
export function isClaudeTokenExpiring(credentialsJson: string, now: number = Date.now()): boolean {
  const oauth = parseClaudeOAuthBlock(credentialsJson);
  if (!oauth) {
    return false;
  }

  const expiresAt = oauth.expiresAt;
  if (typeof expiresAt !== "number" || !Number.isFinite(expiresAt)) {
    return true;
  }

  return now + TOKEN_EXPIRY_BUFFER_MS >= expiresAt;
}

/**
 * 토큰 응답을 기존 자격증명에 덮어씌운 JSON을 돌려준다.
 *
 * `claudeAiOauth` 밖의 키(MCP 서버 토큰 등)와 응답이 주지 않은 필드를 모두 보존한다.
 * refresh 토큰은 서버가 새 값을 준 경우에만 교체한다 — 회전값을 흘리면 다음 갱신이 실패한다.
 */
export function applyRefreshedClaudeToken(
  credentialsJson: string,
  response: ClaudeTokenResponse,
  now: number = Date.now(),
): string | null {
  let parsed: ClaudeCredentials;
  try {
    parsed = JSON.parse(credentialsJson) as ClaudeCredentials;
  } catch {
    return null;
  }

  const accessToken = response.access_token;
  if (typeof accessToken !== "string" || !accessToken.trim()) {
    return null;
  }

  const oauth: ClaudeOAuthBlock = { ...parsed.claudeAiOauth };
  oauth.accessToken = accessToken;

  if (typeof response.expires_in === "number" && Number.isFinite(response.expires_in)) {
    oauth.expiresAt = now + response.expires_in * 1000;
  }

  if (typeof response.refresh_token === "string" && response.refresh_token.trim()) {
    oauth.refreshToken = response.refresh_token;
  }

  if (typeof response.scope === "string" && response.scope.trim()) {
    oauth.scopes = response.scope.split(" ");
  }

  parsed.claudeAiOauth = oauth;
  return JSON.stringify(parsed);
}

/**
 * 저장된 refresh 토큰으로 액세스 토큰을 갱신한 자격증명 JSON을 돌려준다.
 *
 * 실패하면 예외 대신 null을 돌려준다. 호출부는 null을 "기존 자격증명을 그대로 쓴다"로 다뤄야 한다 —
 * 일시적인 429나 네트워크 오류가 사용자의 로그인을 깨뜨리는 결과로 번지면 안 된다.
 */
export async function refreshClaudeCredentials(
  credentialsJson: string,
  now: number = Date.now(),
): Promise<string | null> {
  const refreshToken = readRefreshToken(credentialsJson);
  if (!refreshToken) {
    return null;
  }

  try {
    const response = await fetch(CLAUDE_OAUTH_TOKEN_URL, {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({
        grant_type: "refresh_token",
        refresh_token: refreshToken,
        client_id: CLAUDE_OAUTH_CLIENT_ID,
      }).toString(),
      signal: AbortSignal.timeout(AI_USAGE_REQUEST_TIMEOUT_MS),
    });

    if (!response.ok) {
      // 상태 코드만 남긴다. 토큰은 어떤 경우에도 로그에 담지 않는다
      console.warn(`[ai-usage] Claude 토큰 갱신 실패: HTTP ${response.status}`);
      return null;
    }

    return applyRefreshedClaudeToken(credentialsJson, (await response.json()) as ClaudeTokenResponse, now);
  } catch (error) {
    console.warn("[ai-usage] Claude 토큰 갱신 요청 실패:", error instanceof Error ? error.message : error);
    return null;
  }
}
