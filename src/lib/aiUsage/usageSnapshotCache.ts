import { createHash } from "crypto";
import type {
  AiUsageAccountResult,
  AiUsageSnapshot,
  AiUsageWindow,
} from "@/lib/aiUsage/types";

/** 저장 형식이 바뀌면 올린다. 값이 다른 캐시는 해석하지 않고 버린다 */
const CACHE_SCHEMA_VERSION = 1;

const CACHED_ACCOUNT_ID_LENGTH = 16;

const KNOWN_PROVIDERS = new Set(["claude", "codex", "gemini"]);
const KNOWN_STATUSES = new Set(["ok", "unavailable", "error"]);
const KNOWN_WINDOW_KINDS = new Set(["session", "weekly", "monthly", "model"]);

interface CachedSnapshotEnvelope {
  version: number;
  snapshot: AiUsageSnapshot;
}

/**
 * 계정 UUID와 config dir 경로는 화면이 쓰지 않는데 디스크에는 남는다.
 * 카드를 이어 붙이는 데 필요한 건 "같은 계정인지"뿐이라 되돌릴 수 없는 요약값으로 바꿔 저장한다.
 */
export function toCachedAccountId(accountId: string): string {
  return createHash("sha256").update(accountId).digest("hex").slice(0, CACHED_ACCOUNT_ID_LENGTH);
}

function toCacheableWindow(usageWindow: AiUsageWindow): AiUsageWindow {
  return {
    kind: usageWindow.kind,
    modelName: usageWindow.modelName,
    usedPercent: usageWindow.usedPercent,
    resetsAt: usageWindow.resetsAt,
  };
}

/** 필드를 하나씩 골라 담는다. 나중에 결과 타입에 토큰 같은 값이 붙어도 캐시로 새어 나가지 않는다 */
function toCacheableAccount(account: AiUsageAccountResult): AiUsageAccountResult {
  return {
    provider: account.provider,
    accountId: toCachedAccountId(account.accountId),
    label: account.label,
    status: account.status,
    planName: account.planName,
    windows: account.windows.map(toCacheableWindow),
    reason: account.reason,
    fetchedAt: account.fetchedAt,
  };
}

export function toCacheableSnapshot(snapshot: AiUsageSnapshot): string {
  const envelope: CachedSnapshotEnvelope = {
    version: CACHE_SCHEMA_VERSION,
    snapshot: {
      accounts: snapshot.accounts.map(toCacheableAccount),
      fetchedAt: snapshot.fetchedAt,
    },
  };

  return JSON.stringify(envelope);
}

function isCachedWindow(value: unknown): value is AiUsageWindow {
  const usageWindow = value as AiUsageWindow | null;
  return (
    typeof usageWindow === "object"
    && usageWindow !== null
    && KNOWN_WINDOW_KINDS.has(usageWindow.kind)
    && typeof usageWindow.usedPercent === "number"
    && (usageWindow.modelName === null || typeof usageWindow.modelName === "string")
    && (usageWindow.resetsAt === null || typeof usageWindow.resetsAt === "string")
  );
}

function isCachedAccount(value: unknown): value is AiUsageAccountResult {
  const account = value as AiUsageAccountResult | null;
  return (
    typeof account === "object"
    && account !== null
    && KNOWN_PROVIDERS.has(account.provider)
    && KNOWN_STATUSES.has(account.status)
    && typeof account.accountId === "string"
    && typeof account.label === "string"
    && typeof account.fetchedAt === "string"
    && Array.isArray(account.windows)
    && account.windows.every(isCachedWindow)
  );
}

/**
 * 저장된 스냅샷을 복원한다.
 *
 * 캐시는 화면을 빨리 채우기 위한 편의값이므로, 조금이라도 해석이 어긋나면 고쳐 쓰지 않고 버린다.
 */
export function fromCachedSnapshot(raw: string | null): AiUsageSnapshot | null {
  if (!raw) {
    return null;
  }

  let envelope: CachedSnapshotEnvelope;
  try {
    envelope = JSON.parse(raw) as CachedSnapshotEnvelope;
  } catch {
    return null;
  }

  if (envelope?.version !== CACHE_SCHEMA_VERSION) {
    return null;
  }

  const snapshot = envelope.snapshot;
  if (
    typeof snapshot !== "object"
    || snapshot === null
    || typeof snapshot.fetchedAt !== "string"
    || !Array.isArray(snapshot.accounts)
    || !snapshot.accounts.every(isCachedAccount)
  ) {
    return null;
  }

  return snapshot;
}
