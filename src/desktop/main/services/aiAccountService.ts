import { mkdir, rm } from "fs/promises";
import { getAppSetting, setAppSetting } from "@/desktop/main/services/appSettingsService";
import {
  addAccountRegistration,
  parseAccountRegistrations,
  removeAccountRegistration,
  serializeAccountRegistrations,
  type AiAccountRegistration,
} from "@/lib/aiUsage/accountRegistry";
import { discoverProviderAccounts, hasStoredCredentials } from "@/lib/aiUsage/accountDiscovery";
import { logoutThroughCli, readProviderAuthStatus } from "@/lib/aiUsage/providerCli";
import {
  AI_PROVIDER_CONFIG_DIR_SPECS,
  isValidAccountName,
  toAccountConfigDir,
  toAccountNameFromRoot,
  toNamedAccountRoot,
} from "@/lib/aiUsage/providerConfigDir";
import type { AiUsageAccount, AiUsageProvider } from "@/lib/aiUsage/types";

/** KanVibe에서 만든 AI 계정 목록. 로그아웃된 계정도 자리를 지키게 하는 유일한 근거다 */
const AI_ACCOUNTS_KEY = "ai_accounts";

const AI_USAGE_PROVIDERS: AiUsageProvider[] = ["claude", "codex", "gemini"];

export interface AiAccountSummary {
  provider: AiUsageProvider;
  /** CLI에 계정 위치를 알릴 때 넘기는 루트이자 이 목록에서 계정을 가리키는 식별자 */
  accountRoot: string;
  /** 사용자가 붙인 계정 이름. 홈의 기본 계정은 이름이 없다 */
  accountName: string | null;
  label: string;
  isLoggedIn: boolean;
  planName: string | null;
  /** KanVibe가 만든 계정만 앱에서 지울 수 있다 */
  isRemovable: boolean;
}

export type AiAccountMutationResult =
  | { outcome: "ok" }
  | { outcome: "invalid-name" }
  | { outcome: "already-exists" }
  | { outcome: "manual-logout-required" };

export async function getAiAccountRegistrations(): Promise<AiAccountRegistration[]> {
  return parseAccountRegistrations(await getAppSetting(AI_ACCOUNTS_KEY));
}

async function saveAiAccountRegistrations(
  registrations: AiAccountRegistration[],
): Promise<void> {
  await setAppSetting(AI_ACCOUNTS_KEY, serializeAccountRegistrations(registrations));
}

/**
 * CLI가 알려주는 상태를 우선하고, 물을 수 없는 provider만 자격증명 파일로 판단한다.
 * CLI가 답하지 못한 경우는 "로그아웃"이 아니라 "모른다"이므로 파일 쪽 판단을 덮어쓰지 않는다.
 */
async function toAccountSummary(
  account: AiUsageAccount,
  registeredRoots: Set<string>,
): Promise<AiAccountSummary> {
  const spec = AI_PROVIDER_CONFIG_DIR_SPECS[account.provider];
  const authStatus = await readProviderAuthStatus(account.provider, account.accountRoot);
  const isLoggedIn = authStatus?.isLoggedIn
    ?? (await hasStoredCredentials(spec, account.configDir));

  return {
    provider: account.provider,
    accountRoot: account.accountRoot,
    accountName: toAccountNameFromRoot(spec, account.accountRoot),
    label: authStatus?.label ?? account.label,
    isLoggedIn,
    planName: authStatus?.planName ?? null,
    isRemovable: registeredRoots.has(account.accountRoot),
  };
}

/** 화면 순서는 provider 순서가 정한다. 조회할 때마다 카드가 자리를 바꾸면 눈이 매번 다시 찾는다 */
export async function listAiAccounts(): Promise<AiAccountSummary[]> {
  const registrations = await getAiAccountRegistrations();
  const registeredRoots = new Set(
    registrations.map((registration) => registration.accountRoot),
  );

  const discoveredAccounts = await Promise.all(
    AI_USAGE_PROVIDERS.map((provider) => discoverProviderAccounts(provider, registrations)),
  );

  return Promise.all(
    discoveredAccounts.flat().map((account) => toAccountSummary(account, registeredRoots)),
  );
}

/**
 * 계정 자리를 먼저 만들고 등록한다. 실제 로그인은 이어서 열리는 로그인 세션이 CLI에게 맡긴다.
 *
 * 디렉터리를 미리 만들어 두는 것은 CLI가 그 자리를 자기 것으로 쓰게 하기 위해서다.
 */
export async function addAiAccount(
  provider: AiUsageProvider,
  accountName: string,
): Promise<AiAccountMutationResult & { accountRoot?: string }> {
  if (!isValidAccountName(accountName)) {
    return { outcome: "invalid-name" };
  }

  const spec = AI_PROVIDER_CONFIG_DIR_SPECS[provider];
  const accountRoot = toNamedAccountRoot(spec, accountName);
  const registrations = await getAiAccountRegistrations();
  if (registrations.some((registration) => registration.accountRoot === accountRoot)) {
    return { outcome: "already-exists" };
  }

  await mkdir(toAccountConfigDir(spec, accountRoot), { recursive: true });
  await saveAiAccountRegistrations(
    addAccountRegistration(registrations, provider, accountName),
  );

  return { outcome: "ok", accountRoot };
}

/**
 * 계정을 지운다.
 *
 * 로그아웃은 CLI에게 맡기고, KanVibe가 만든 계정 디렉터리만 지운다.
 * 사용자가 직접 만든 형제 디렉터리는 KanVibe의 것이 아니므로 로그아웃까지만 하고 남겨 둔다.
 */
export async function removeAiAccount(
  provider: AiUsageProvider,
  accountRoot: string,
): Promise<AiAccountMutationResult> {
  const registrations = await getAiAccountRegistrations();
  const isRegistered = registrations.some(
    (registration) => registration.accountRoot === accountRoot,
  );

  const didLogout = await logoutThroughCli(provider, accountRoot);

  if (isRegistered) {
    await rm(accountRoot, { recursive: true, force: true });
    await saveAiAccountRegistrations(removeAccountRegistration(registrations, accountRoot));
    return { outcome: "ok" };
  }

  return didLogout ? { outcome: "ok" } : { outcome: "manual-logout-required" };
}
