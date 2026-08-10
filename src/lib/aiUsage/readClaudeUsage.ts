import path from "path";
import {
  AI_USAGE_REQUEST_TIMEOUT_MS,
  classifyUsageHttpFailure,
  createErrorUsage,
  createUnavailableUsage,
  createUsageResult,
  createUsageWindow,
} from "@/lib/aiUsage/shared";
import type { AiUsageProviderResult, AiUsageWindow, AiUsageWindowKind } from "@/lib/aiUsage/types";
import { getHomeDirectory, readTextFile } from "@/lib/hostFileAccess";

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
 * 로컬에 적힌 만료 시각은 이 엔드포인트의 판단 기준이 아니다.
 * 만료로 표시된 자격증명이 여전히 통과하는 경우가 있어 서버 응답으로만 유효성을 가른다.
 */
async function readClaudeAccessToken(): Promise<string | null> {
  const homeDirectory = await getHomeDirectory();
  const rawCredentials = await readTextFile(
    path.join(homeDirectory, ".claude", ".credentials.json"),
  );
  if (!rawCredentials) {
    return null;
  }

  try {
    const parsed = JSON.parse(rawCredentials) as { claudeAiOauth?: { accessToken?: unknown } };
    const accessToken = parsed?.claudeAiOauth?.accessToken;
    return typeof accessToken === "string" && accessToken.trim() ? accessToken : null;
  } catch {
    return null;
  }
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

export async function readClaudeUsage(): Promise<AiUsageProviderResult> {
  const accessToken = await readClaudeAccessToken();
  if (!accessToken) {
    return createUnavailableUsage("claude", "missing-credentials");
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
    return createErrorUsage("claude", "fetch-failed");
  }

  if (!response.ok) {
    return classifyUsageHttpFailure("claude", response.status);
  }

  let payload: ClaudeUsageResponse;
  try {
    payload = (await response.json()) as ClaudeUsageResponse;
  } catch {
    return createErrorUsage("claude", "fetch-failed");
  }

  const windows = [
    toClaudeUsageWindow(payload.five_hour, "session"),
    toClaudeUsageWindow(payload.seven_day, "weekly"),
  ].filter((window): window is AiUsageWindow => window !== null);

  if (windows.length === 0) {
    return createErrorUsage("claude", "empty-response");
  }

  return createUsageResult("claude", windows);
}
