import {
  discoverClaudeAccounts,
  discoverCodexAccounts,
  discoverGeminiAccounts,
} from "@/lib/aiUsage/accountDiscovery";
import { readClaudeUsage } from "@/lib/aiUsage/readClaudeUsage";
import { readCodexUsage } from "@/lib/aiUsage/readCodexUsage";
import { readGeminiUsage } from "@/lib/aiUsage/readGeminiUsage";
import { createErrorUsage, createUnavailableUsage } from "@/lib/aiUsage/shared";
import type {
  AiUsageAccount,
  AiUsageAccountResult,
  AiUsageProvider,
  AiUsageSnapshot,
} from "@/lib/aiUsage/types";

interface ProviderUsageSource {
  provider: AiUsageProvider;
  discover: () => Promise<AiUsageAccount[]>;
  read: (account: AiUsageAccount) => Promise<AiUsageAccountResult>;
}

/** 화면 순서는 이 배열이 정한다. 조회 결과에 따라 카드 순서가 바뀌면 눈이 매번 다시 찾아야 한다 */
const PROVIDER_SOURCES: ProviderUsageSource[] = [
  { provider: "claude", discover: discoverClaudeAccounts, read: readClaudeUsage },
  { provider: "codex", discover: discoverCodexAccounts, read: readCodexUsage },
  { provider: "gemini", discover: discoverGeminiAccounts, read: readGeminiUsage },
];

/**
 * 로그인한 계정이 없는 provider를 대신할 자리표시자.
 * 카드까지 사라지면 사용자는 "지원하지 않는다"와 "로그인하면 보인다"를 구분할 수 없다.
 */
function createSignedOutAccount(provider: AiUsageProvider): AiUsageAccount {
  return { provider, accountId: provider, label: provider, configDir: "" };
}

async function discoverAccountsOrNone(source: ProviderUsageSource): Promise<AiUsageAccount[]> {
  try {
    return await source.discover();
  } catch {
    // 한 provider의 탐색 실패가 나머지 provider의 사용량까지 가려서는 안 된다
    return [];
  }
}

interface AccountUsageTask {
  account: AiUsageAccount;
  read: () => Promise<AiUsageAccountResult>;
}

function buildUsageTasks(
  source: ProviderUsageSource,
  accounts: AiUsageAccount[],
): AccountUsageTask[] {
  if (accounts.length === 0) {
    const signedOutAccount = createSignedOutAccount(source.provider);
    return [{
      account: signedOutAccount,
      read: async () => createUnavailableUsage(signedOutAccount, "missing-credentials"),
    }];
  }

  return accounts.map((account) => ({ account, read: () => source.read(account) }));
}

/**
 * 등록된 모든 계정의 사용량을 한 번에 모은다.
 *
 * 계정 하나의 실패가 나머지를 가리지 않도록 결과를 따로 받아 실패한 자리에만 error를 채운다.
 */
export async function aggregateAiUsage(): Promise<AiUsageSnapshot> {
  const discoveredAccounts = await Promise.all(PROVIDER_SOURCES.map(discoverAccountsOrNone));
  const usageTasks = PROVIDER_SOURCES.flatMap((source, index) => (
    buildUsageTasks(source, discoveredAccounts[index])
  ));

  const settledResults = await Promise.allSettled(usageTasks.map(({ read }) => read()));
  const accounts = settledResults.map((settled, index) => (
    settled.status === "fulfilled"
      ? settled.value
      : createErrorUsage(usageTasks[index].account, "fetch-failed")
  ));

  return {
    accounts,
    fetchedAt: new Date().toISOString(),
  };
}
