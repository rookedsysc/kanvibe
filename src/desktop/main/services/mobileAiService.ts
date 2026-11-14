import { createHash } from "node:crypto";
import { addAiAccount, listAiAccounts, removeAiAccount, type AiAccountSummary } from "@/desktop/main/services/aiAccountService";
import { getAiUsageSnapshot } from "@/desktop/main/services/aiUsageService";
import { getTaskById } from "@/desktop/main/services/kanbanService";
import { getTaskAgentCallGraph, getTaskLiveAiSessions, getRunningAgentPanes } from "@/desktop/main/services/projectService";
import { closeMobileAiLoginsForAccount, openMobileAiLoginSession, type MobileAiLoginCallbacks } from "@/desktop/main/mobileAiLoginBridge";
import { isAiSessionProvider } from "@/lib/aiSessions/providers";
import { isValidAccountName } from "@/lib/aiUsage/providerConfigDir";
import type { AiUsageProvider, AiUsageSnapshot } from "@/lib/aiUsage/types";

export { closeAllMobileAiLogins, closeMobileAiLogin, closeMobileAiLoginsForDevice, resizeMobileAiLogin, writeMobileAiLogin } from "@/desktop/main/mobileAiLoginBridge";

const MAX_IDENTIFIER_LENGTH = 512;
const ACCOUNT_ID_PATTERN = /^[a-f0-9]{64}$/;
const ACCOUNT_PROVIDERS = new Set(["claude", "codex", "gemini"]);

// 삭제 중에 시작한 로그인 조회가 지워진 계정의 CLI를 다시 띄우지 않도록 두 진입점을 직렬화한다.
let accountOperationQueue: Promise<unknown> = Promise.resolve();
function serializeAccountOperation<Result>(operation: () => Promise<Result>): Promise<Result> {
  const result = accountOperationQueue.then(operation);
  accountOperationQueue = result.catch(() => undefined);
  return result;
}

class MobileAiRequestError extends Error {
  constructor(readonly statusCode: number, message: string) {
    super(message);
    this.name = "MobileAiRequestError";
  }
}

function validateIdentifier(value: unknown): asserts value is string {
  if (typeof value !== "string" || !value.trim() || value.length > MAX_IDENTIFIER_LENGTH || /[\x00-\x1f]/.test(value)) {
    throw new MobileAiRequestError(400, "식별자가 올바르지 않습니다");
  }
}

function accountHandle(provider: string, accountRoot: string): string {
  return createHash("sha256").update(JSON.stringify([provider, accountRoot])).digest("hex");
}

function publicAccount(account: AiAccountSummary) {
  return {
    accountId: accountHandle(account.provider, account.accountRoot),
    provider: account.provider,
    accountName: account.accountName,
    label: account.label,
    isLoggedIn: account.isLoggedIn,
    planName: account.planName,
    isRemovable: account.isRemovable,
  };
}

async function findMobileAccount(accountId: unknown): Promise<AiAccountSummary> {
  if (typeof accountId !== "string" || !ACCOUNT_ID_PATTERN.test(accountId)) {
    throw new MobileAiRequestError(400, "계정 식별자가 올바르지 않습니다");
  }
  const account = (await listAiAccounts()).find((candidate) => accountHandle(candidate.provider, candidate.accountRoot) === accountId);
  if (!account) throw new MobileAiRequestError(404, "계정을 찾을 수 없습니다");
  return account;
}

async function requireTask(taskId: unknown): Promise<string> {
  validateIdentifier(taskId);
  if (!await getTaskById(taskId)) throw new MobileAiRequestError(404, "태스크를 찾을 수 없습니다");
  return taskId;
}

export async function getMobileLiveAiSessions(taskId: string) {
  return getTaskLiveAiSessions(await requireTask(taskId));
}

export async function getMobileAgentCallGraph(taskId: string, provider: unknown, sessionId: unknown) {
  if (!isAiSessionProvider(provider)) throw new MobileAiRequestError(400, "지원하지 않는 AI 제공자입니다");
  validateIdentifier(sessionId);
  if (/[\\/]/.test(sessionId) || sessionId === "." || sessionId === "..") {
    throw new MobileAiRequestError(400, "AI 세션 식별자가 올바르지 않습니다");
  }
  return getTaskAgentCallGraph(await requireTask(taskId), provider, sessionId);
}

export async function listMobileAiAccounts() {
  return (await listAiAccounts()).map(publicAccount);
}

export async function addMobileAiAccount(input: unknown): Promise<{ accountId: string }> {
  if (!input || typeof input !== "object" || Array.isArray(input)) throw new MobileAiRequestError(400, "계정 정보를 확인해 주세요");
  const values = input as Record<string, unknown>;
  if (Object.keys(values).some((key) => !["provider", "accountName"].includes(key))
    || typeof values.provider !== "string" || !ACCOUNT_PROVIDERS.has(values.provider)
    || typeof values.accountName !== "string" || !isValidAccountName(values.accountName)) {
    throw new MobileAiRequestError(400, "제공자와 계정 이름을 확인해 주세요");
  }
  const provider = values.provider as AiUsageProvider;
  const result = await addAiAccount(provider, values.accountName);
  if (result.outcome === "already-exists") throw new MobileAiRequestError(409, "이미 등록된 계정 이름입니다");
  if (result.outcome !== "ok" || !result.accountRoot) throw new MobileAiRequestError(400, "계정을 추가하지 못했습니다");
  return { accountId: accountHandle(provider, result.accountRoot) };
}

export async function removeMobileAiAccount(accountId: string): Promise<{ outcome: "ok" }> {
  return serializeAccountOperation(async () => {
    const account = await findMobileAccount(accountId);
    if (!account.isRemovable) throw new MobileAiRequestError(403, "KanVibe에서 추가한 계정만 삭제할 수 있습니다");
    closeMobileAiLoginsForAccount(account.accountRoot);
    const result = await removeAiAccount(account.provider, account.accountRoot);
    if (result.outcome !== "ok") throw new MobileAiRequestError(409, "PC에서 계정 로그아웃을 먼저 완료해 주세요");
    return { outcome: "ok" as const };
  });
}

export async function getMobileAiUsageSnapshot(): Promise<AiUsageSnapshot> {
  const snapshot = await getAiUsageSnapshot();
  return {
    fetchedAt: snapshot.fetchedAt,
    accounts: snapshot.accounts.map((account) => ({
      provider: account.provider,
      // 사용량의 원본 ID는 provider UUID나 디스크 경로라 관리용 handle과 별개로 요약한다.
      accountId: accountHandle(account.provider, account.accountId),
      label: account.label,
      status: account.status,
      planName: account.planName,
      reason: account.reason,
      fetchedAt: account.fetchedAt,
      windows: account.windows.map((window) => ({ kind: window.kind, modelName: window.modelName, usedPercent: window.usedPercent, resetsAt: window.resetsAt })),
    })),
  };
}

export async function openMobileAiLogin(deviceId: string, socketId: string, accountId: string, cols: number, rows: number, callbacks: MobileAiLoginCallbacks): Promise<void> {
  return serializeAccountOperation(async () => {
    const account = await findMobileAccount(accountId);
    if (!callbacks.isOpen()) return;
    await openMobileAiLoginSession({ deviceId, socketId, provider: account.provider, accountRoot: account.accountRoot, cols, rows, ...callbacks });
  });
}

export async function getMobileRunningAgentPanes() {
  return getRunningAgentPanes();
}
