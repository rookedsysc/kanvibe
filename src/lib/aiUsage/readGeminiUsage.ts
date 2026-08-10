import path from "path";
import {
  loadGeminiProjectId,
  refreshGeminiAccessToken,
  resolveGeminiOAuthClient,
} from "@/lib/aiUsage/geminiOAuthClient";
import {
  AI_USAGE_REQUEST_TIMEOUT_MS,
  classifyUsageHttpFailure,
  createErrorUsage,
  createUnavailableUsage,
  createUsageResult,
  createUsageWindow,
} from "@/lib/aiUsage/shared";
import type { AiUsageAccount, AiUsageAccountResult, AiUsageWindow } from "@/lib/aiUsage/types";
import { readTextFile } from "@/lib/hostFileAccess";

const RETRIEVE_USER_QUOTA_URL = "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota";

/** 응답의 모델 id는 기계용이라 그대로 두면 패널이 읽기 어렵다 */
const GEMINI_MODEL_DISPLAY_NAMES: Record<string, string> = {
  "gemini-3.1-pro": "3.1 Pro",
  "gemini-3.1-flash": "3.1 Flash",
  "gemini-3.0-pro": "3.0 Pro",
  "gemini-3.0-flash": "3.0 Flash",
  "gemini-2.5-pro": "Pro",
  "gemini-2.5-flash": "Flash",
  "gemini-2.5-flash-lite": "Flash Lite",
};

interface GeminiQuotaBucket {
  remainingFraction: number;
  modelId: string;
  resetTime: string;
}

interface GeminiCredentials {
  accessToken: string;
  refreshToken: string;
  expiryDate: number;
}

function toModelDisplayName(modelId: string): string {
  const mappedName = GEMINI_MODEL_DISPLAY_NAMES[modelId];
  if (mappedName) {
    return mappedName;
  }

  return modelId
    .replace(/^gemini-/i, "")
    .split("-")
    .map((part) => (part ? part[0].toUpperCase() + part.slice(1) : part))
    .join(" ");
}

/** 응답은 남은 비율을 주는데 다른 provider와 나란히 보이려면 사용한 비율이어야 한다 */
function toGeminiUsageWindow(bucket: GeminiQuotaBucket): AiUsageWindow | null {
  return createUsageWindow(
    "model",
    Math.round((1 - bucket.remainingFraction) * 100),
    bucket.resetTime,
    toModelDisplayName(bucket.modelId),
  );
}

function isQuotaBucket(value: unknown): value is GeminiQuotaBucket {
  const bucket = value as GeminiQuotaBucket | null;
  return (
    typeof bucket === "object"
    && bucket !== null
    && typeof bucket.remainingFraction === "number"
    && Number.isFinite(bucket.remainingFraction)
    && typeof bucket.modelId === "string"
    && typeof bucket.resetTime === "string"
  );
}

/** 이 엔드포인트는 버킷 배열을 최상위로 주기도 하고 buckets 키에 담아 주기도 한다 */
function parseQuotaBuckets(payload: unknown): GeminiQuotaBucket[] {
  if (Array.isArray(payload)) {
    return payload.filter(isQuotaBucket);
  }

  const bucketsField = (payload as { buckets?: unknown } | null)?.buckets;
  return Array.isArray(bucketsField) ? bucketsField.filter(isQuotaBucket) : [];
}

async function readGeminiCredentials(configDir: string): Promise<GeminiCredentials | null> {
  const rawCredentials = await readTextFile(path.join(configDir, "oauth_creds.json"));
  if (!rawCredentials) {
    return null;
  }

  try {
    const parsed = JSON.parse(rawCredentials) as {
      access_token?: unknown;
      refresh_token?: unknown;
      expiry_date?: unknown;
    };
    if (typeof parsed.access_token !== "string" || !parsed.access_token.trim()) {
      return null;
    }

    return {
      accessToken: parsed.access_token,
      refreshToken: typeof parsed.refresh_token === "string" ? parsed.refresh_token : "",
      expiryDate: typeof parsed.expiry_date === "number" ? parsed.expiry_date : 0,
    };
  } catch {
    return null;
  }
}

/**
 * 갱신한 토큰을 oauth_creds.json에 되쓰지 않는다.
 * 그 파일은 Gemini CLI의 것이고, KanVibe가 남의 도구 설정을 고치면 CLI 쪽 상태와 어긋날 수 있다.
 */
async function resolveAccessToken(
  account: AiUsageAccount,
  credentials: GeminiCredentials,
): Promise<string | AiUsageAccountResult> {
  if (credentials.expiryDate > Date.now()) {
    return credentials.accessToken;
  }

  const oauthClient = await resolveGeminiOAuthClient();
  if (!oauthClient) {
    return createUnavailableUsage(account, "gemini-cli-not-found");
  }

  const refreshed = await refreshGeminiAccessToken(credentials.refreshToken, oauthClient);
  if (!refreshed) {
    return createUnavailableUsage(account, "expired-credentials");
  }

  return refreshed.accessToken;
}

export async function readGeminiUsage(account: AiUsageAccount): Promise<AiUsageAccountResult> {
  const credentials = await readGeminiCredentials(account.configDir);
  if (!credentials) {
    return createUnavailableUsage(account, "missing-credentials");
  }

  const resolvedToken = await resolveAccessToken(account, credentials);
  if (typeof resolvedToken !== "string") {
    return resolvedToken;
  }

  const projectId = await loadGeminiProjectId(resolvedToken);
  if (!projectId) {
    return createErrorUsage(account, "fetch-failed");
  }

  let response: Response;
  try {
    response = await fetch(RETRIEVE_USER_QUOTA_URL, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${resolvedToken}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ project: projectId }),
      signal: AbortSignal.timeout(AI_USAGE_REQUEST_TIMEOUT_MS),
    });
  } catch {
    return createErrorUsage(account, "fetch-failed");
  }

  if (!response.ok) {
    return classifyUsageHttpFailure(account, response.status);
  }

  let payload: unknown;
  try {
    payload = await response.json();
  } catch {
    return createErrorUsage(account, "fetch-failed");
  }

  const windows = parseQuotaBuckets(payload)
    .map(toGeminiUsageWindow)
    .filter((window): window is AiUsageWindow => window !== null);

  if (windows.length === 0) {
    return createErrorUsage(account, "empty-response");
  }

  return createUsageResult(account, windows);
}
