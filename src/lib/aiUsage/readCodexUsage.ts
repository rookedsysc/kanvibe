import path from "path";
import {
  AI_USAGE_REQUEST_TIMEOUT_MS,
  classifyUsageHttpFailure,
  createErrorUsage,
  createUnavailableUsage,
  createUsageResult,
  createUsageWindow,
} from "@/lib/aiUsage/shared";
import type {
  AiUsageAccount,
  AiUsageAccountResult,
  AiUsageWindow,
  AiUsageWindowKind,
} from "@/lib/aiUsage/types";
import { readTextFile } from "@/lib/hostFileAccess";

const CODEX_USAGE_URL = "https://chatgpt.com/backend-api/wham/usage";

const CODEX_SESSION_WINDOW_SECONDS = 18_000;
const CODEX_WEEKLY_WINDOW_SECONDS = 604_800;

/** 무료 등급은 5시간·7일 창 대신 30일 창 하나만 받는다 */
const CODEX_MONTHLY_WINDOW_SECONDS = 2_592_000;

/** 예전 응답에서 관측된 1분 남짓의 창 길이 흔들림만 흡수하고 다른 길이는 받아들이지 않는다 */
const CODEX_WINDOW_TOLERANCE_SECONDS = 60;

interface CodexRateWindowResponse {
  used_percent?: unknown;
  limit_window_seconds?: unknown;
  reset_at?: unknown;
}

interface CodexUsageResponse {
  plan_type?: unknown;
  rate_limit?: {
    primary_window?: CodexRateWindowResponse | null;
    secondary_window?: CodexRateWindowResponse | null;
  } | null;
}

interface CodexCredentials {
  accessToken: string;
  accountId: string | null;
}

async function readCodexCredentials(configDir: string): Promise<CodexCredentials | null> {
  const rawAuth = await readTextFile(path.join(configDir, "auth.json"));
  if (!rawAuth) {
    return null;
  }

  try {
    const parsed = JSON.parse(rawAuth) as {
      tokens?: { access_token?: unknown; account_id?: unknown } | null;
    };
    const accessToken = parsed?.tokens?.access_token;
    if (typeof accessToken !== "string" || !accessToken.trim()) {
      return null;
    }

    const accountId = parsed?.tokens?.account_id;
    return {
      accessToken,
      accountId: typeof accountId === "string" && accountId.trim() ? accountId : null,
    };
  } catch {
    return null;
  }
}

/**
 * 창의 종류는 primary·secondary 순서가 아니라 창 길이가 정한다.
 * 실제 응답에서 primary_window가 7일 창으로 오는 계정이 있어, 순서로 판정하면 5시간 한도를 잘못 표시한다.
 */
function classifyCodexWindowKind(limitWindowSeconds: unknown): AiUsageWindowKind | null {
  if (typeof limitWindowSeconds !== "number" || !Number.isFinite(limitWindowSeconds)) {
    return null;
  }

  if (Math.abs(limitWindowSeconds - CODEX_SESSION_WINDOW_SECONDS) <= CODEX_WINDOW_TOLERANCE_SECONDS) {
    return "session";
  }

  if (Math.abs(limitWindowSeconds - CODEX_WEEKLY_WINDOW_SECONDS) <= CODEX_WINDOW_TOLERANCE_SECONDS) {
    return "weekly";
  }

  if (Math.abs(limitWindowSeconds - CODEX_MONTHLY_WINDOW_SECONDS) <= CODEX_WINDOW_TOLERANCE_SECONDS) {
    return "monthly";
  }

  return null;
}

function toCodexUsageWindow(raw: CodexRateWindowResponse | null | undefined): AiUsageWindow | null {
  if (!raw) {
    return null;
  }

  const kind = classifyCodexWindowKind(raw.limit_window_seconds);
  if (!kind) {
    return null;
  }

  return createUsageWindow(kind, raw.used_percent, raw.reset_at);
}

export async function readCodexUsage(account: AiUsageAccount): Promise<AiUsageAccountResult> {
  const credentials = await readCodexCredentials(account.configDir);
  if (!credentials) {
    return createUnavailableUsage(account, "missing-credentials");
  }

  const headers: Record<string, string> = {
    Authorization: `Bearer ${credentials.accessToken}`,
  };
  if (credentials.accountId) {
    headers["ChatGPT-Account-Id"] = credentials.accountId;
  }

  let response: Response;
  try {
    response = await fetch(CODEX_USAGE_URL, {
      headers,
      signal: AbortSignal.timeout(AI_USAGE_REQUEST_TIMEOUT_MS),
    });
  } catch {
    return createErrorUsage(account, "fetch-failed");
  }

  if (!response.ok) {
    return classifyUsageHttpFailure(account, response.status);
  }

  let payload: CodexUsageResponse;
  try {
    payload = (await response.json()) as CodexUsageResponse;
  } catch {
    return createErrorUsage(account, "fetch-failed");
  }

  const mappedWindows = [
    toCodexUsageWindow(payload.rate_limit?.primary_window),
    toCodexUsageWindow(payload.rate_limit?.secondary_window),
  ].filter((window): window is AiUsageWindow => window !== null);

  // 응답이 어느 순서로 오든 짧은 창을 먼저 보여줘야 어느 한도가 먼저 닫히는지 바로 읽힌다
  const windows = (["session", "weekly", "monthly"] as const)
    .map((kind) => mappedWindows.find((window) => window.kind === kind))
    .filter((window): window is AiUsageWindow => window !== undefined);

  if (windows.length === 0) {
    return createErrorUsage(account, "empty-response");
  }

  const planName = typeof payload.plan_type === "string" ? payload.plan_type : null;
  return createUsageResult(account, windows, planName);
}
